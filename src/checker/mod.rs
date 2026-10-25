//! Iterative scan orchestration and the reusable public Checker API.
//!
//! The outer loop suspends/resumes annotation scans. The inner loop keeps a live
//! lexer and applies budgets and structural rules to each token before continuing.

mod annotations;
#[cfg(test)]
mod fast_path_tests;
mod scratch;
#[cfg(test)]
mod tests;
mod widths;

use crate::lexer::{Kind, Lexer, LexerState};
use crate::stack::{SCAN_FRAMES, Stack};
use crate::{ConfigError, Limits, Reason, Rejection};
use scratch::{SavedScratch, ScanScratch, annotation_frames};
use std::ops::Range;

/// Rejects source that exceeds the configured structural budgets.
/// Successful screening does not validate Python syntax.
/// Uses scratch that grows on demand within configured bounds.
/// Use [`Checker`] to reuse scratch, or [`Checker::preallocated`] for allocation-free scans.
pub fn check(source: &str, limits: &Limits) -> Result<(), Rejection> {
    if limits.validate().is_err() {
        return Err(Rejection {
            reason: Reason::Configuration,
            range: 0..0,
        });
    }
    if source.len() > limits.max_source_bytes {
        return Err(Rejection {
            reason: Reason::SourceSize,
            range: 0..source.len(),
        });
    }

    Checker::new_validated(limits.clone()).check(source)
}

/// Reusable screening state backed by bounded, shared scratch buffers.
/// [`Checker::new`] defers scratch allocation until the first scan.
/// [`Checker::preallocated`] reserves it during construction instead.
/// Default buffers grow on demand up to configured bounds and retain capacity.
///
/// Use one checker per worker when screening many inputs before parsing them.
/// Source strings are borrowed only for the duration of each check.
pub struct Checker {
    limits: Limits,
    buffers: Option<CheckBuffers>,
}

struct CheckBuffers {
    scratch: ScanScratch,
    work: Stack<ScanFrame, SCAN_FRAMES>,
}

impl Checker {
    /// Creates a checker whose buffers start empty and grow on demand within
    /// configured bounds. Checks retain capacity for subsequent inputs.
    pub fn new(limits: Limits) -> Result<Self, ConfigError> {
        limits.validate()?;
        Ok(Self::new_validated(limits))
    }

    fn new_validated(limits: Limits) -> Self {
        Self {
            limits,
            buffers: None,
        }
    }

    /// Reserves scratch storage immediately, including for the first check.
    /// Input cannot grow these buffers; checks perform no heap allocations.
    pub fn preallocated(limits: Limits) -> Result<Self, ConfigError> {
        let mut checker = Self::new(limits)?;
        checker.buffers = Some(CheckBuffers::new(&checker.limits, true));
        Ok(checker)
    }

    /// Statement-level proofs with local detailed fallback.
    /// Returns the same rejection reasons and ranges as normal screening.
    #[cfg(feature = "regions")]
    pub fn check_regions(&mut self, source: &str) -> Result<(), Rejection> {
        self.check_mode::<true>(source)
    }

    /// Rejects source that exceeds the configured structural budgets.
    /// Successful screening does not validate Python syntax.
    /// Each scan resets logical contents while retaining buffer capacities,
    /// including when the preceding scan rejected its input.
    pub fn check(&mut self, source: &str) -> Result<(), Rejection> {
        self.check_mode::<false>(source)
    }

    fn check_mode<const REGION_PROOFS: bool>(&mut self, source: &str) -> Result<(), Rejection> {
        if source.len() > self.limits.max_source_bytes {
            return Err(Rejection {
                reason: Reason::SourceSize,
                range: 0..source.len(),
            });
        }

        // Oversized source is rejected before initializing even the growing buffers.
        let buffers = self
            .buffers
            .get_or_insert_with(|| CheckBuffers::new(&self.limits, false));
        scan::<true, REGION_PROOFS>(
            source,
            &self.limits,
            &mut buffers.scratch,
            &mut buffers.work,
        )
    }
}

impl CheckBuffers {
    fn new(limits: &Limits, preallocated: bool) -> Self {
        Self {
            scratch: ScanScratch::new(limits, preallocated),
            work: Stack::with_storage(annotation_frames(limits), preallocated),
        }
    }
}

// One frame describes a source slice, never an owned copy. Nested annotation
// frames borrow the parent's remaining depth rather than starting fresh budgets.
#[derive(Default)]
struct ScanFrame {
    source: Range<usize>,
    budget: u32,
    ast_budget: Option<u32>,
    annotation_source: bool,
    depth: u8,
    // None starts a new scan; Some resumes immediately after a quoted string.
    lexer: Option<LexerState>,
    // Only suspended parents hold saved state; their buffers stay in place.
    saved: Option<SavedScratch>,
    region_start: bool,
    outer_annotation: Option<Range<usize>>,
}

impl ScanFrame {
    #[inline]
    fn absolute_range(&self, local: &Range<usize>) -> Range<usize> {
        self.source.start + local.start..self.source.start + local.end
    }

    #[inline(always)]
    fn rejection(&self, reason: Reason, local: &Range<usize>) -> Rejection {
        // A child scans a slice, but callers need a range in their original
        // source. Attribute structural failures to the enclosing annotation;
        // preserve Inconclusive because budget exhaustion proves no violation.
        Rejection {
            reason: if self.outer_annotation.is_some() && reason != Reason::Inconclusive {
                Reason::QuotedAnnotation
            } else {
                reason
            },
            range: self
                .outer_annotation
                .clone()
                .unwrap_or_else(|| self.absolute_range(local)),
        }
    }
}

// Const generics compile disabled fast paths out of the scan loops instead of
// checking runtime flags for each token. Tests disable SIMPLE_LINES to compare
// the shortcut with detailed scanning; REGION_PROOFS selects statement proofs.
fn scan<const SIMPLE_LINES: bool, const REGION_PROOFS: bool>(
    source: &str,
    limits: &Limits,
    scratch: &mut ScanScratch,
    work: &mut Stack<ScanFrame, SCAN_FRAMES>,
) -> Result<(), Rejection> {
    // Select once per input so disabled width budgets add no per-token checks.
    if limits.widths_enabled() {
        scan_tokens::<SIMPLE_LINES, REGION_PROOFS, true>(source, limits, scratch, work)
    } else {
        scan_tokens::<SIMPLE_LINES, REGION_PROOFS, false>(source, limits, scratch, work)
    }
}

fn scan_tokens<const SIMPLE_LINES: bool, const REGION_PROOFS: bool, const COUNT_WIDTHS: bool>(
    source: &str,
    limits: &Limits,
    scratch: &mut ScanScratch,
    work: &mut Stack<ScanFrame, SCAN_FRAMES>,
) -> Result<(), Rejection> {
    // Reset all segment bases after success or rejection, including child errors.
    scratch.clear_segments();
    work.clear();
    work.push(ScanFrame {
        source: 0..source.len(),
        budget: u32::from(limits.max_nesting_depth),
        ast_budget: limits.max_ast_depth.map(u32::from),
        annotation_source: false,
        depth: 0,
        lexer: None,
        saved: None,
        region_start: true,
        outer_annotation: None,
    });
    let mut tokens_used = 0usize;
    // Charge the outer source once, then charge annotation contents again when
    // reinterpreted. Nested strings must not multiply the configured work budget.
    let mut bytes_scanned = source.len();
    if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
        return Err(Rejection {
            reason: Reason::Inconclusive,
            range: 0..source.len(),
        });
    }

    loop {
        let Some(index) = work.len().checked_sub(1) else {
            return Ok(());
        };
        let current = &mut work[index];
        let frame_source = &source[current.source.clone()];
        let ScanScratch {
            lexer: lexer_scratch,
            scanner,
            annotations,
            widths,
        } = &mut *scratch;

        // Parent entries remain below the active segment in the shared buffers;
        // each frame retains its scalar tracking state.
        let mut lexer = if let Some(state) = current.lexer {
            Lexer::resume(
                frame_source,
                current.budget as usize,
                usize::from(limits.max_indentation_depth),
                lexer_scratch,
                state,
            )
        } else {
            scanner.reset(current.budget, current.ast_budget);
            annotations.reset();
            widths.reset();

            let mut lexer = Lexer::new(
                frame_source,
                current.budget as usize,
                usize::from(limits.max_indentation_depth),
                lexer_scratch,
            );
            if SIMPLE_LINES {
                let simple_budget = current
                    .budget
                    .min(current.ast_budget.unwrap_or(current.budget))
                    as usize;
                while let Some(count) = lexer.skip_simple_line(
                    simple_budget,
                    limits.max_tokens.saturating_sub(tokens_used),
                    limits.max_token_bytes,
                ) {
                    tokens_used += count;
                    if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                        return Err(Rejection {
                            reason: Reason::Inconclusive,
                            range: current.source.clone(),
                        });
                    }
                }
            }
            lexer
        };

        // Keep this frame's lexer live until an annotation suspends it. Saving
        // and rebuilding scalar state for every token adds work to normal code.
        let next_child = loop {
            #[cfg(feature = "regions")]
            if REGION_PROOFS
                && !limits.widths_enabled()
                && current.region_start
                && annotations.region_ready()
                && let Some(remaining) = scanner.region_budget()
                && let Some((count, seen_code)) = lexer.skip_region(
                    remaining,
                    limits.max_tokens.saturating_sub(tokens_used),
                    limits.max_token_bytes,
                    limits.check_string_annotations,
                    current.annotation_source,
                )
            {
                tokens_used += count;
                if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                    return Err(Rejection {
                        reason: Reason::Inconclusive,
                        range: current.source.clone(),
                    });
                }
                scanner.finish_region(seen_code);
                annotations.reset();
                continue;
            }

            let Some(token) = lexer.next() else {
                break None;
            };
            current.region_start =
                matches!(token.kind, Kind::Newline | Kind::Indent | Kind::Dedent);
            tokens_used += 1;

            if tokens_used > limits.max_tokens {
                return Err(current.rejection(Reason::TokenCount, &token.range));
            }
            if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                return Err(current.rejection(Reason::Inconclusive, &token.range));
            }
            if limits
                .max_token_bytes
                .is_some_and(|limit| token.range.len() > limit)
            {
                return Err(current.rejection(Reason::TokenSize, &token.range));
            }
            let lexical_reason = match token.kind {
                Kind::Limit => Some(Reason::LexicalStructure),
                Kind::NestingLimit => Some(Reason::Nesting),
                Kind::IndentationLimit => Some(Reason::Indentation),
                _ => None,
            };
            if let Some(reason) = lexical_reason {
                return Err(current.rejection(reason, &token.range));
            }
            // Even without width limits, retain bounded frame tracking: unfinished
            // lambda parameters can fill scratch before parser depth is charged.
            let width_reason = if COUNT_WIDTHS {
                widths.observe(frame_source, token.kind, &token.range, limits)
            } else {
                widths.observe_capacity(frame_source, token.kind, &token.range)
            };
            if let Some(reason) = width_reason {
                return Err(current.rejection(reason, &token.range));
            }

            let mut child = None;
            if limits.check_string_annotations
                && (current.annotation_source || annotations.active())
                && annotations.type_position()
            {
                if let Some(contents) = token.contents.clone() {
                    let remaining = current.budget.saturating_sub(scanner.total);
                    let ast_remaining = current
                        .ast_budget
                        .map(|budget| budget.saturating_sub(scanner.ast_total));
                    let minimum = remaining.min(ast_remaining.unwrap_or(remaining));
                    // Without decoding or expansion, byte length bounds the
                    // possible structural depth. Short contents need no rescan.
                    if contents.len() > minimum as usize {
                        if current.depth >= limits.max_annotation_depth {
                            return Err(current.rejection(Reason::QuotedAnnotation, &token.range));
                        }
                        let Some(next_bytes) = bytes_scanned.checked_add(contents.len()) else {
                            return Err(current.rejection(Reason::QuotedAnnotation, &token.range));
                        };
                        if next_bytes > limits.max_source_bytes {
                            return Err(current.rejection(Reason::QuotedAnnotation, &token.range));
                        }
                        bytes_scanned = next_bytes;
                        if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                            return Err(current.rejection(Reason::Inconclusive, &token.range));
                        }
                        child = Some(ScanFrame {
                            source: current.absolute_range(&contents),
                            budget: remaining,
                            ast_budget: ast_remaining,
                            annotation_source: true,
                            depth: current.depth + 1,
                            lexer: None,
                            saved: None,
                            region_start: true,
                            outer_annotation: Some(
                                current
                                    .outer_annotation
                                    .clone()
                                    .unwrap_or_else(|| current.absolute_range(&token.range)),
                            ),
                        });
                    }
                }
            }
            if let Some(reason) = scanner.observe(token.kind) {
                return Err(current.rejection(reason, &token.range));
            }
            annotations.observe(frame_source, token.kind, &token.range);
            if let Some(child) = child {
                // Observe the string in its parent before suspending: the child
                // borrows the remaining depth but has independent lexical state.
                current.lexer = Some(lexer.state());
                break Some(child);
            }
        };

        if let Some(child) = next_child {
            work[index].saved = Some(scratch.suspend());
            work.push(child);
        } else {
            work.pop();
            if let Some(parent) = work.last_mut() {
                scratch.restore(parent.saved.take().expect("parent suspended before child"));
            }
        }
    }
}

fn work_exceeded(bytes_scanned: usize, tokens_used: usize, limit: usize) -> bool {
    bytes_scanned.saturating_add(tokens_used) > limit
}
