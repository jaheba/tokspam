//! Iterative scan orchestration and the reusable public Checker API.
//!
//! The outer loop suspends/resumes annotation scans. The inner loop keeps a live
//! lexer and applies budgets and structural rules to each token before continuing.

mod annotations;
mod escapes;
#[cfg(test)]
mod fast_path_tests;
mod scratch;
#[cfg(test)]
mod tests;
mod widths;

use crate::lexer::{Kind, Lexer, LexerState};
use crate::stack::Stack;
use crate::{Limits, Reason, Rejection, Report, Scores, Storage, StorageError};
use scratch::{SavedScratch, ScanScratch};
use std::ops::Range;

/// Fast early-exit checking with caller-selected score thresholds.
pub fn check(source: &str, limits: &Limits) -> Result<(), Rejection> {
    Checker::new(limits.clone()).check(source)
}

/// Collect all scores without score thresholds or a fixed storage ceiling.
pub fn analyze(source: &str) -> Report {
    Checker::default().analyze(source)
}

/// Reusable complexity measurement and checking. Source is never retained.
/// Growing scratch retains capacity; fixed scratch performs no scan allocations.
#[derive(Default)]
pub struct Checker {
    limits: Limits,
    buffers: Option<CheckBuffers>,
}

struct CheckBuffers {
    scratch: ScanScratch,
    work: Stack<ScanFrame>,
}

impl Checker {
    /// Create a growing checker without allocating scratch.
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            buffers: None,
        }
    }

    /// Reserve a fixed number of entries per stack, independently of limits.
    /// Reservation failure is reported before any input is scanned.
    pub fn with_storage(limits: Limits, storage: Storage) -> Result<Self, StorageError> {
        let buffers = CheckBuffers::new(storage);
        if buffers.scratch.failed() || buffers.work.failed() {
            return Err(StorageError);
        }
        Ok(Self {
            limits,
            buffers: Some(buffers),
        })
    }

    /// Collect scores until completion or the first configured threshold/resource
    /// stop. Scores in an incomplete report describe only the observed prefix.
    pub fn analyze(&mut self, source: &str) -> Report {
        let mut scores = Scores {
            source_bytes: source.len(),
            ..Scores::default()
        };
        let stopped = self.run::<false, true>(source, &mut scores).err();
        Report { scores, stopped }
    }

    /// Apply thresholds with early exits; no full score report is collected.
    /// Success does not validate Python syntax.
    pub fn check(&mut self, source: &str) -> Result<(), Rejection> {
        self.run::<false, false>(source, &mut Scores::default())
    }

    /// Enable conservative statement proofs with detailed fallback.
    #[cfg(feature = "regions")]
    pub fn check_regions(&mut self, source: &str) -> Result<(), Rejection> {
        self.run::<true, false>(source, &mut Scores::default())
    }

    fn run<const REGION_PROOFS: bool, const COLLECT: bool>(
        &mut self,
        source: &str,
        scores: &mut Scores,
    ) -> Result<(), Rejection> {
        if self
            .limits
            .max_source_bytes
            .is_some_and(|limit| source.len() > limit)
        {
            return Err(Rejection {
                reason: Reason::SourceSize,
                range: 0..source.len(),
            });
        }
        // Structural charges are bounded by a constant per scanned byte. Check
        // representability before arithmetic, including cumulative child rescans.
        if source.len().checked_mul(16).is_none() {
            return Err(Rejection {
                reason: Reason::Overflow,
                range: 0..source.len(),
            });
        }
        let buffers = self
            .buffers
            .get_or_insert_with(|| CheckBuffers::new(Storage::Growing));
        if COLLECT {
            scan_tokens::<false, false, true, true>(
                source,
                &self.limits,
                &mut buffers.scratch,
                &mut buffers.work,
                scores,
            )
        } else {
            scan::<true, REGION_PROOFS>(
                source,
                &self.limits,
                &mut buffers.scratch,
                &mut buffers.work,
            )
        }
    }
}

impl CheckBuffers {
    fn new(storage: Storage) -> Self {
        let (capacity, fixed) = match storage {
            Storage::Growing => (0, false),
            Storage::Fixed { capacity } => (capacity, true),
        };
        Self {
            scratch: ScanScratch::new(storage),
            work: Stack::with_storage(capacity, fixed),
        }
    }
}

// One frame describes a source slice, never an owned copy. Nested annotation
// frames borrow the parent's remaining depth rather than starting fresh budgets.
#[derive(Default)]
struct ScanFrame {
    source: Range<usize>,
    budget: usize,
    ast_budget: Option<usize>,
    annotation_source: bool,
    // Source offsets address either caller input or the shared decoded arena.
    decoded: bool,
    needs_decode: bool,
    decoded_mark: Option<usize>,
    depth: usize,
    parser_offset: usize,
    ast_offset: usize,
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
            reason: if self.outer_annotation.is_some()
                && !matches!(
                    reason,
                    Reason::Inconclusive
                        | Reason::Storage
                        | Reason::Overflow
                        | Reason::LexicalStructure
                        | Reason::AnnotationEscape
                ) {
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
    work: &mut Stack<ScanFrame>,
) -> Result<(), Rejection> {
    // Select once per input so disabled width budgets add no per-token checks.
    if limits.widths_enabled() {
        scan_tokens::<SIMPLE_LINES, REGION_PROOFS, true, false>(
            source,
            limits,
            scratch,
            work,
            &mut Scores::default(),
        )
    } else {
        scan_tokens::<SIMPLE_LINES, REGION_PROOFS, false, false>(
            source,
            limits,
            scratch,
            work,
            &mut Scores::default(),
        )
    }
}

fn scan_tokens<
    const SIMPLE_LINES: bool,
    const REGION_PROOFS: bool,
    const COUNT_WIDTHS: bool,
    const COLLECT: bool,
>(
    source: &str,
    limits: &Limits,
    scratch: &mut ScanScratch,
    work: &mut Stack<ScanFrame>,
    scores: &mut Scores,
) -> Result<(), Rejection> {
    // Reset all segment bases after success or rejection, including child errors.
    scratch.clear_segments();
    work.clear();
    if !work.push(ScanFrame {
        source: 0..source.len(),
        budget: limits.max_nesting_depth.unwrap_or(usize::MAX),
        ast_budget: if COLLECT {
            Some(limits.max_ast_depth.unwrap_or(usize::MAX))
        } else {
            limits.max_ast_depth
        },
        annotation_source: false,
        decoded: false,
        needs_decode: false,
        decoded_mark: None,
        depth: 0,
        parser_offset: 0,
        ast_offset: 0,
        lexer: None,
        saved: None,
        region_start: true,
        outer_annotation: None,
    }) {
        return Err(Rejection {
            reason: Reason::Storage,
            range: 0..0,
        });
    }
    let mut tokens_used = 0usize;
    // Charge the outer source once, then charge annotation contents again when
    // reinterpreted. Nested strings must not multiply the configured work budget.
    let mut bytes_scanned = source.len();
    if COLLECT {
        scores.scanned_bytes = bytes_scanned;
        scores.work_units = bytes_scanned;
    }
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
        let ScanScratch {
            decoded,
            lexer: lexer_scratch,
            scanner,
            annotations,
            widths,
        } = &mut *scratch;
        let frame_source = if current.decoded {
            &decoded[current.source.clone()]
        } else {
            &source[current.source.clone()]
        };

        // Parent entries remain below the active segment in the shared buffers;
        // each frame retains its scalar tracking state.
        let mut lexer = if let Some(state) = current.lexer {
            Lexer::resume(
                frame_source,
                limits.max_delimiter_depth.unwrap_or(usize::MAX),
                limits.max_indentation_depth.unwrap_or(usize::MAX),
                lexer_scratch,
                state,
            )
        } else {
            if !scanner.reset(current.budget, current.ast_budget) {
                return Err(current.rejection(Reason::Storage, &(0..0)));
            }
            annotations.reset();
            widths.reset();

            let mut lexer = Lexer::new(
                frame_source,
                limits.max_delimiter_depth.unwrap_or(usize::MAX),
                limits.max_indentation_depth.unwrap_or(usize::MAX),
                lexer_scratch,
            );
            if SIMPLE_LINES {
                let simple_budget = current
                    .budget
                    .min(current.ast_budget.unwrap_or(current.budget));
                while let Some(count) = lexer.skip_simple_line(
                    simple_budget,
                    limits
                        .max_tokens
                        .unwrap_or(usize::MAX)
                        .saturating_sub(tokens_used),
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
                    limits
                        .max_tokens
                        .unwrap_or(usize::MAX)
                        .saturating_sub(tokens_used),
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

            if COLLECT {
                scores.tokens = tokens_used;
                scores.work_units = bytes_scanned.saturating_add(tokens_used);
                scores.max_token_bytes = scores.max_token_bytes.max(token.range.len());
                let (delimiters, indents) = lexer.depths();
                scores.delimiter_depth = scores.delimiter_depth.max(delimiters);
                scores.indentation_depth = scores.indentation_depth.max(indents);
                // Lexer thresholds reject before pushing an overflowing level.
                // Include that observed attempt, while storage failures prove no depth.
                if token.kind == Kind::NestingLimit && !lexer.storage_failed() {
                    scores.delimiter_depth = scores.delimiter_depth.max(delimiters + 1);
                }
                if token.kind == Kind::IndentationLimit {
                    scores.indentation_depth = scores.indentation_depth.max(indents + 1);
                }
            }
            // The lexer has already attempted the push. Fixed storage exhaustion
            // is incomplete measurement, even when this token also crosses a score limit.
            if token.kind == Kind::StorageLimit
                || (token.kind == Kind::NestingLimit && lexer.storage_failed())
            {
                return Err(current.rejection(Reason::Storage, &token.range));
            }
            if limits.max_tokens.is_some_and(|limit| tokens_used > limit) {
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
                Kind::NestingLimit => Some(Reason::DelimiterDepth),
                Kind::IndentationLimit => Some(Reason::Indentation),
                _ => None,
            };
            if let Some(reason) = lexical_reason {
                return Err(current.rejection(reason, &token.range));
            }
            // Collection always needs widths. Checking compiles tracking out
            // completely when no width threshold can affect its result.
            let width_reason = if COUNT_WIDTHS {
                widths.observe::<COLLECT>(frame_source, token.kind, &token.range, limits, scores)
            } else {
                None
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
                    // possible structural depth. Skip short contents only when
                    // no other requested threshold needs their tokens or levels.
                    if COLLECT
                        || contents.len() > minimum
                        || limits.max_annotation_depth.is_some()
                        || limits.max_tokens.is_some()
                        || limits.max_work_units.is_some()
                        || limits.max_token_bytes.is_some()
                        || limits.widths_enabled()
                        || limits.max_delimiter_depth.is_some()
                        || limits.max_indentation_depth.is_some()
                    {
                        if COLLECT {
                            scores.annotation_depth =
                                scores.annotation_depth.max(current.depth + 1);
                        }
                        if limits
                            .max_annotation_depth
                            .is_some_and(|limit| current.depth >= limit)
                        {
                            return Err(current.rejection(Reason::QuotedAnnotation, &token.range));
                        }
                        let Some(next_bytes) = bytes_scanned.checked_add(contents.len()) else {
                            return Err(current.rejection(Reason::Overflow, &token.range));
                        };
                        if next_bytes.checked_mul(16).is_none() {
                            return Err(current.rejection(Reason::Overflow, &token.range));
                        }
                        bytes_scanned = next_bytes;
                        if COLLECT {
                            scores.scanned_bytes = bytes_scanned;
                            scores.work_units = bytes_scanned.saturating_add(tokens_used);
                        }
                        if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                            return Err(current.rejection(Reason::Inconclusive, &token.range));
                        }
                        child = Some(ScanFrame {
                            source: current.absolute_range(&contents),
                            budget: remaining,
                            ast_budget: ast_remaining,
                            annotation_source: true,
                            decoded: current.decoded,
                            needs_decode: token.needs_decode,
                            decoded_mark: None,
                            depth: current.depth + 1,
                            parser_offset: current.parser_offset + scanner.total,
                            ast_offset: current.ast_offset + scanner.ast_total,
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
            let structural_reason = scanner.observe::<COLLECT>(token.kind);
            if COLLECT {
                scores.parser_depth = scores
                    .parser_depth
                    .max(current.parser_offset + scanner.peak_parser);
                scores.ast_depth = scores.ast_depth.max(current.ast_offset + scanner.peak_ast);
            }
            if let Some(reason) = structural_reason {
                return Err(current.rejection(reason, &token.range));
            }
            if let Some(reason) = annotations.observe(frame_source, token.kind, &token.range) {
                return Err(current.rejection(reason, &token.range));
            }
            if let Some(child) = child {
                // Observe the string in its parent before suspending: the child
                // borrows the remaining depth but has independent lexical state.
                current.lexer = Some(lexer.state());
                break Some(child);
            }
        };

        if let Some(mut child) = next_child {
            if child.needs_decode {
                let mark = decoded.len();
                child.source =
                    escapes::decode(source, child.source.clone(), child.decoded, decoded).map_err(
                        |reason| Rejection {
                            reason,
                            range: child
                                .outer_annotation
                                .clone()
                                .expect("annotation frame has an outer range"),
                        },
                    )?;
                child.decoded = true;
                child.needs_decode = false;
                child.decoded_mark = Some(mark);
                // The existing charge covers the raw decoding pass. Charge the
                // decoded source separately before its token scan can begin.
                bytes_scanned = bytes_scanned
                    .checked_add(child.source.len())
                    .ok_or_else(|| child.rejection(Reason::Overflow, &(0..0)))?;
                if bytes_scanned.checked_mul(16).is_none() {
                    return Err(child.rejection(Reason::Overflow, &(0..0)));
                }
                if COLLECT {
                    scores.scanned_bytes = bytes_scanned;
                    scores.work_units = bytes_scanned + tokens_used;
                }
                if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                    return Err(child.rejection(Reason::Inconclusive, &(0..0)));
                }
            }
            work[index].saved = Some(scratch.suspend());
            if !work.push(child) {
                return Err(Rejection {
                    reason: Reason::Storage,
                    range: work[index]
                        .outer_annotation
                        .clone()
                        .unwrap_or_else(|| work[index].source.clone()),
                });
            }
        } else {
            let finished = work.pop().expect("active scan frame");
            if let Some(mark) = finished.decoded_mark {
                decoded.truncate(mark);
            }
            if let Some(parent) = work.last_mut() {
                scratch.restore(parent.saved.take().expect("parent suspended before child"));
            }
        }
    }
}

fn work_exceeded(bytes_scanned: usize, tokens_used: usize, limit: Option<usize>) -> bool {
    limit.is_some_and(|limit| bytes_scanned.saturating_add(tokens_used) > limit)
}
