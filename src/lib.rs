//! Experimental pre-parse screening with an independent lexer.
//! Acceptance is not yet a verified bound on Python parser recursion.
//!
//! ```
//! use tokspam::{Limits, check};
//! let limits = Limits { max_nesting_depth: 80, ..Limits::default() };
//! assert!(check("x = [1, 2, 3]", &limits).is_ok());
//! ```
mod lexer;
mod rules;
mod stack;

use lexer::{Kind, Lexer, LexerScratch, LexerState};
use rules::{Scanner, ScannerState};
use stack::{Mark, SCAN_FRAMES, STRUCTURAL_CAPACITY, Stack};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    SourceSize,
    TokenCount,
    TokenSize,
    Nesting,
    Indentation,
    AstDepth,
    LexicalStructure,
    QuotedAnnotation,
    Configuration,
    CallArguments,
    Parameters,
    DisplayItems,
    SliceItems,
    InterpolationFields,
    /// Screening stopped because the configured work budget was exhausted.
    Inconclusive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub reason: Reason,
    pub range: Range<usize>,
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} at bytes {}..{}",
            self.reason, self.range.start, self.range.end
        )
    }
}

impl std::error::Error for Rejection {}

pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TOKENS: usize = 1_000_000;
pub const MAX_WORK_UNITS: usize = MAX_SOURCE_BYTES + MAX_TOKENS;
pub const MAX_TOKEN_BYTES: usize = MAX_SOURCE_BYTES;
pub const MAX_NESTING_DEPTH: u16 = 1_024;
pub const MAX_INDENTATION_DEPTH: u16 = 1_024;
pub const MAX_ANNOTATION_DEPTH: u8 = 32;
pub const MAX_AST_DEPTH: u16 = 4_096;
pub const MAX_CONSTRUCT_WIDTH: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    SourceBytes,
    Tokens,
    TokenBytes,
    NestingDepth,
    IndentationDepth,
    AnnotationDepth,
    AstDepth,
    CallArguments,
    Parameters,
    DisplayItems,
    SliceItems,
    InterpolationFields,
    WorkUnits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub limit: LimitKind,
    pub configured: usize,
    pub maximum: usize,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} limit {} exceeds hard maximum {}",
            self.limit, self.configured, self.maximum
        )
    }
}

impl std::error::Error for ConfigError {}

/// Screening budgets. These are heuristic limits, not parser safety guarantees.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_source_bytes: usize,
    pub max_tokens: usize,
    /// Maximum cumulative scanned bytes plus emitted tokens.
    pub max_work_units: usize,
    pub max_token_bytes: Option<usize>,
    pub max_nesting_depth: u16,
    pub max_indentation_depth: u16,
    pub max_annotation_depth: u8,
    pub check_string_annotations: bool,
    /// Flat AST-shape estimates supplement the parser-recursion rules.
    pub max_ast_depth: Option<u16>,
    /// Experimental flat-width budgets. `None` preserves the normal scanner path.
    pub max_call_arguments: Option<usize>,
    pub max_parameters: Option<usize>,
    pub max_display_items: Option<usize>,
    pub max_slice_items: Option<usize>,
    pub max_interpolation_fields: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            max_tokens: 100_000,
            max_work_units: 1024 * 1024 + 100_000,
            max_token_bytes: None,
            max_nesting_depth: 100,
            max_indentation_depth: 80,
            max_annotation_depth: 8,
            check_string_annotations: true,
            max_ast_depth: Some(100),
            max_call_arguments: None,
            max_parameters: None,
            max_display_items: None,
            max_slice_items: None,
            max_interpolation_fields: None,
        }
    }
}

impl Limits {
    /// Larger byte and token budgets for screening published package sources.
    /// Structural limits match `Default`; this is not a parser-safety guarantee.
    pub fn package_sources() -> Self {
        Self {
            max_source_bytes: 8 * 1024 * 1024,
            max_tokens: 500_000,
            max_work_units: 8 * 1024 * 1024 + 500_000,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_limit(
            self.max_source_bytes,
            MAX_SOURCE_BYTES,
            LimitKind::SourceBytes,
        )?;
        validate_limit(self.max_tokens, MAX_TOKENS, LimitKind::Tokens)?;
        validate_limit(self.max_work_units, MAX_WORK_UNITS, LimitKind::WorkUnits)?;
        if let Some(value) = self.max_token_bytes {
            validate_limit(value, MAX_TOKEN_BYTES, LimitKind::TokenBytes)?;
        }
        validate_limit(
            usize::from(self.max_nesting_depth),
            usize::from(MAX_NESTING_DEPTH),
            LimitKind::NestingDepth,
        )?;
        validate_limit(
            usize::from(self.max_indentation_depth),
            usize::from(MAX_INDENTATION_DEPTH),
            LimitKind::IndentationDepth,
        )?;
        validate_limit(
            usize::from(self.max_annotation_depth),
            usize::from(MAX_ANNOTATION_DEPTH),
            LimitKind::AnnotationDepth,
        )?;
        if let Some(value) = self.max_ast_depth {
            validate_limit(
                usize::from(value),
                usize::from(MAX_AST_DEPTH),
                LimitKind::AstDepth,
            )?;
        }
        for (value, kind) in [
            (self.max_call_arguments, LimitKind::CallArguments),
            (self.max_parameters, LimitKind::Parameters),
            (self.max_display_items, LimitKind::DisplayItems),
            (self.max_slice_items, LimitKind::SliceItems),
            (
                self.max_interpolation_fields,
                LimitKind::InterpolationFields,
            ),
        ] {
            if let Some(value) = value {
                validate_limit(value, MAX_CONSTRUCT_WIDTH, kind)?;
            }
        }
        Ok(())
    }
}

fn validate_limit(value: usize, maximum: usize, limit: LimitKind) -> Result<(), ConfigError> {
    if value > maximum {
        Err(ConfigError {
            limit,
            configured: value,
            maximum,
        })
    } else {
        Ok(())
    }
}

/// Rejects source that exceeds the configured structural budgets.
/// Successful screening does not validate Python syntax.
/// Allocates bounded scratch before scanning; scanning performs no heap allocations.
/// Use [`Checker`] to reuse that scratch across multiple inputs.
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
/// Construction reserves storage from the configured limits; subsequent checks
/// perform no heap allocations.
///
/// Use one checker per worker when screening many inputs before parsing them.
/// Source strings are borrowed only for the duration of each check.
pub struct Checker {
    limits: Limits,
    scratch: ScanScratch,
    work: Stack<ScanWork, SCAN_FRAMES>,
}

impl Checker {
    pub fn new(limits: Limits) -> Result<Self, ConfigError> {
        limits.validate()?;
        Ok(Self::new_validated(limits))
    }

    fn new_validated(limits: Limits) -> Self {
        let frames = annotation_frames(&limits);

        Self {
            scratch: ScanScratch::new(&limits),
            work: Stack::with_capacity(frames),
            limits,
        }
    }

    /// Experimental statement-level proofs with local detailed fallback.
    /// Returns the same rejection reasons and ranges as normal screening.
    #[cfg(feature = "experimental-regions")]
    pub fn check_regions(&mut self, source: &str) -> Result<(), Rejection> {
        self.check_mode::<true>(source)
    }

    /// Rejects source that exceeds the configured structural budgets.
    /// Successful screening does not validate Python syntax.
    pub fn check(&mut self, source: &str) -> Result<(), Rejection> {
        self.check_mode::<false>(source)
    }

    fn check_mode<const REGIONS: bool>(&mut self, source: &str) -> Result<(), Rejection> {
        if source.len() > self.limits.max_source_bytes {
            return Err(Rejection {
                reason: Reason::SourceSize,
                range: 0..source.len(),
            });
        }

        scan::<true, REGIONS>(source, &self.limits, &mut self.scratch, &mut self.work)
    }
}

#[derive(Default)]
struct ScanWork {
    source: Range<usize>,
    budget: u32,
    ast_budget: Option<u32>,
    annotation_source: bool,
    depth: u8,
    lexer: Option<LexerState>,
    saved: Option<SavedScratch>,
    region_start: bool,
    outer_annotation: Option<Range<usize>>,
}

// One set of shared buffers serves every scan. Work frames save only scalar
// state and segment marks; suspended entries remain below the active base.
struct ScanScratch {
    lexer: LexerScratch,
    scanner: Scanner,
    annotations: AnnotationTracker,
    widths: WidthTracker,
}

struct SavedScratch {
    lexer: [Mark; 2],
    scanner: (ScannerState, [Mark; 3]),
    annotations: (AnnotationState, Mark),
    widths: (WidthState, Mark),
}

impl ScanScratch {
    fn suspend(&mut self) -> SavedScratch {
        SavedScratch {
            lexer: self.lexer.suspend(),
            scanner: self.scanner.suspend(),
            annotations: (self.annotations.state, self.annotations.brackets.suspend()),
            widths: (self.widths.state, self.widths.frames.suspend()),
        }
    }

    fn restore(&mut self, saved: SavedScratch) {
        self.lexer.restore(saved.lexer);
        self.scanner.restore(saved.scanner);
        self.annotations.state = saved.annotations.0;
        self.annotations.brackets.restore(saved.annotations.1);
        self.widths.state = saved.widths.0;
        self.widths.frames.restore(saved.widths.1);
    }

    fn clear_segments(&mut self) {
        self.lexer.clear_segments();
        self.scanner.clear_segments();
        self.annotations.brackets.reset();
        self.widths.frames.reset();
    }

    fn new(limits: &Limits) -> Self {
        let frames = annotation_frames(limits);
        let capacity = usize::from(limits.max_nesting_depth) + frames + 1;

        Self {
            lexer: LexerScratch::with_capacity(
                capacity,
                usize::from(limits.max_nesting_depth.min(limits.max_indentation_depth))
                    + frames
                    + 1,
            ),
            scanner: Scanner::with_capacity(
                u32::from(limits.max_nesting_depth),
                limits.max_ast_depth.map(u32::from),
                capacity,
            ),
            annotations: AnnotationTracker {
                brackets: Stack::with_capacity(capacity),
                state: AnnotationState::default(),
            },
            // Pending lambda parameter lists do not charge parser depth until
            // their colons arrive. Preserve the full bounded width capacity.
            widths: WidthTracker::new(),
        }
    }
}

fn annotation_frames(limits: &Limits) -> usize {
    if limits.check_string_annotations {
        usize::from(limits.max_annotation_depth) + 1
    } else {
        1
    }
}

fn scan<const FAST: bool, const REGIONS: bool>(
    source: &str,
    limits: &Limits,
    scratch: &mut ScanScratch,
    work: &mut Stack<ScanWork, SCAN_FRAMES>,
) -> Result<(), Rejection> {
    // Reset all segment bases after success or rejection, including child errors.
    scratch.clear_segments();
    work.clear();
    work.push(ScanWork {
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
            if FAST {
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
            #[cfg(feature = "experimental-regions")]
            if REGIONS
                && !widths_enabled(limits)
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
            let absolute = |range: &Range<usize>| {
                current.source.start + range.start..current.source.start + range.end
            };
            let reject = |reason, range: Range<usize>| {
                Err(Rejection {
                    reason: if current.outer_annotation.is_some() && reason != Reason::Inconclusive
                    {
                        Reason::QuotedAnnotation
                    } else {
                        reason
                    },
                    range: current.outer_annotation.clone().unwrap_or(range),
                })
            };

            if tokens_used > limits.max_tokens {
                return reject(Reason::TokenCount, absolute(&token.range));
            }
            if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                return reject(Reason::Inconclusive, absolute(&token.range));
            }
            if limits
                .max_token_bytes
                .is_some_and(|limit| token.range.len() > limit)
            {
                return reject(Reason::TokenSize, absolute(&token.range));
            }
            let lexical_reason = match token.kind {
                Kind::Limit => Some(Reason::LexicalStructure),
                Kind::NestingLimit => Some(Reason::Nesting),
                Kind::IndentationLimit => Some(Reason::Indentation),
                _ => None,
            };
            if let Some(reason) = lexical_reason {
                return reject(reason, absolute(&token.range));
            }
            if let Some(reason) = widths.observe(frame_source, token.kind, &token.range, limits) {
                return reject(reason, absolute(&token.range));
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
                            return reject(Reason::QuotedAnnotation, absolute(&token.range));
                        }
                        let Some(next_bytes) = bytes_scanned.checked_add(contents.len()) else {
                            return reject(Reason::QuotedAnnotation, absolute(&token.range));
                        };
                        if next_bytes > limits.max_source_bytes {
                            return reject(Reason::QuotedAnnotation, absolute(&token.range));
                        }
                        bytes_scanned = next_bytes;
                        if work_exceeded(bytes_scanned, tokens_used, limits.max_work_units) {
                            return reject(Reason::Inconclusive, absolute(&token.range));
                        }
                        child = Some(ScanWork {
                            source: absolute(&contents),
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
                                    .unwrap_or_else(|| absolute(&token.range)),
                            ),
                        });
                    }
                }
            }
            if let Some(reason) = scanner.observe(token.kind) {
                return reject(reason, absolute(&token.range));
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

#[cfg(feature = "experimental-regions")]
fn widths_enabled(limits: &Limits) -> bool {
    limits.widths_enabled()
}

#[cfg(feature = "experimental-regions")]
impl Limits {
    fn widths_enabled(&self) -> bool {
        self.max_call_arguments.is_some()
            || self.max_parameters.is_some()
            || self.max_display_items.is_some()
            || self.max_slice_items.is_some()
            || self.max_interpolation_fields.is_some()
    }
}

#[derive(Clone, Copy)]
enum WidthKind {
    Call,
    Parameters,
    Display,
    Parenthesized,
    Slice,
    FString,
    Interpolation { in_format: bool },
    Ignore,
}

const _: () = assert!(MAX_TOKENS <= u32::MAX as usize);

struct WidthFrame {
    kind: WidthKind,
    // Counts cannot exceed the shared hard token budget (including malformed
    // input); u32 retains the full range without pointer-sized counters.
    items: u32,
    // Commas finalize items; the closing delimiter charges a nonempty final
    // item. This avoids counting empty constructs or trailing commas twice.
    seen_item: bool,
    has_comma: bool,
    marker_only: bool,
    comprehension: bool,
    adjacent_fields: u32,
}

// Classify delimiters lexically so nested contents count as one parent item.
// This estimates construct widths without validating Python's full grammar.
struct WidthTracker {
    frames: Stack<WidthFrame, STRUCTURAL_CAPACITY>,
    state: WidthState,
}

#[derive(Clone, Copy, Default)]
struct WidthState {
    previous_ends_operand: bool,
    pending_def: bool,
    pending_non_call: bool,
    root_adjacent_fields: u32,
}

impl WidthTracker {
    fn new() -> Self {
        Self {
            frames: Stack::default(),
            state: WidthState::default(),
        }
    }

    fn reset(&mut self) {
        self.frames.clear();
        self.state.previous_ends_operand = false;
        self.state.pending_def = false;
        self.state.pending_non_call = false;
        self.state.root_adjacent_fields = 0;
    }

    fn observe(
        &mut self,
        source: &str,
        kind: Kind,
        range: &Range<usize>,
        limits: &Limits,
    ) -> Option<Reason> {
        if kind == Kind::Name && &source[range.clone()] == "def" {
            self.state.pending_def = true;
        }
        if kind == Kind::Name && matches!(&source[range.clone()], "class" | "import") {
            self.state.pending_non_call = true;
        }

        let opened = match kind {
            Kind::FStringStart => Some(WidthKind::FString),
            Kind::Lambda => Some(WidthKind::Parameters),
            Kind::Lpar if self.state.pending_def => {
                self.state.pending_def = false;
                Some(WidthKind::Parameters)
            }
            Kind::Lpar if self.state.pending_non_call => {
                self.state.pending_non_call = false;
                Some(WidthKind::Ignore)
            }
            Kind::Lpar if self.state.previous_ends_operand => Some(WidthKind::Call),
            Kind::Lpar => Some(WidthKind::Parenthesized),
            Kind::Lsqb if self.state.previous_ends_operand => Some(WidthKind::Slice),
            Kind::Lsqb => Some(WidthKind::Display),
            Kind::Lbrace
                if matches!(
                    self.frames.last().map(|frame| frame.kind),
                    Some(WidthKind::FString | WidthKind::Interpolation { in_format: true })
                ) =>
            {
                if let Some(parent) = self.frames.last_mut() {
                    parent.items += 1;
                    if limits
                        .max_interpolation_fields
                        .is_some_and(|limit| parent.items as usize > limit)
                    {
                        return Some(Reason::InterpolationFields);
                    }
                }
                Some(WidthKind::Interpolation { in_format: false })
            }
            Kind::Lbrace => Some(WidthKind::Display),
            _ => None,
        };

        if let Some(opened) = opened {
            // Unfinished lambda parameter lists can accumulate before their
            // colons charge parser depth. Stop at fixed scratch capacity rather
            // than asserting a structural violation we have not established.
            if self.frames.is_full() {
                return Some(Reason::Inconclusive);
            }

            // Python folds adjacent string literals into one expression. Carry
            // interpolation counts across that boundary, but not other operands.
            let adjacent_fields = if matches!(opened, WidthKind::FString) {
                self.frames
                    .last()
                    .map_or(self.state.root_adjacent_fields, |frame| {
                        frame.adjacent_fields
                    })
            } else {
                if let Some(parent) = self.frames.last_mut() {
                    parent.adjacent_fields = 0;
                } else {
                    self.state.root_adjacent_fields = 0;
                }
                0
            };
            if let Some(parent) = self.frames.last_mut() {
                parent.seen_item = true;
            }
            self.frames.push(WidthFrame {
                kind: opened,
                items: adjacent_fields,
                seen_item: false,
                has_comma: false,
                marker_only: false,
                comprehension: false,
                adjacent_fields: 0,
            });
        } else if kind == Kind::Comma {
            if let Some(frame) = self.frames.last_mut() {
                frame.has_comma = true;
                if frame.seen_item && !frame.marker_only {
                    frame.items += 1;
                }
                frame.seen_item = false;
                frame.marker_only = false;
                if let Some(reason) = frame.exceeded(limits) {
                    return Some(reason);
                }
            }
        } else if matches!(
            kind,
            Kind::Rpar | Kind::Rsqb | Kind::Rbrace | Kind::FStringEnd
        ) || (kind == Kind::Colon
            && matches!(
                self.frames.last().map(|frame| frame.kind),
                Some(WidthKind::Parameters)
            ))
        {
            if let Some(mut frame) = self.frames.pop() {
                if !matches!(
                    frame.kind,
                    WidthKind::FString | WidthKind::Interpolation { .. }
                ) {
                    frame.items += u32::from(frame.seen_item && !frame.marker_only);
                }
                if let Some(reason) = frame.exceeded(limits) {
                    return Some(reason);
                }
                let adjacent_fields = matches!(frame.kind, WidthKind::FString)
                    .then_some(frame.items)
                    .unwrap_or(0);
                if let Some(parent) = self.frames.last_mut() {
                    parent.adjacent_fields = adjacent_fields;
                } else {
                    self.state.root_adjacent_fields = adjacent_fields;
                }
            }
            if let Some(parent) = self.frames.last_mut() {
                parent.seen_item = true;
            }
        } else {
            if kind != Kind::String {
                if let Some(frame) = self.frames.last_mut() {
                    frame.adjacent_fields = 0;
                } else {
                    self.state.root_adjacent_fields = 0;
                }
            }
            if kind == Kind::For
                && let Some(frame) = self.frames.last_mut()
                && matches!(frame.kind, WidthKind::Display)
            {
                frame.comprehension = true;
            }
            if kind == Kind::Colon
                && let Some(WidthFrame {
                    kind: WidthKind::Interpolation { in_format },
                    ..
                }) = self.frames.last_mut()
            {
                *in_format = true;
            }
            if !matches!(
                kind,
                Kind::Indent | Kind::Dedent | Kind::Newline | Kind::Semi
            ) {
                if let Some(frame) = self.frames.last_mut() {
                    if !frame.seen_item
                        && matches!(frame.kind, WidthKind::Parameters)
                        && matches!(kind, Kind::Slash | Kind::Star)
                    {
                        // Bare `/` and `*` separate parameter groups; `*args`
                        // becomes a real item when another token follows.
                        frame.marker_only = true;
                    } else if frame.seen_item {
                        frame.marker_only = false;
                    }
                    frame.seen_item = true;
                }
            }
        }

        if matches!(kind, Kind::Newline | Kind::Semi | Kind::Colon) {
            self.state.pending_def = false;
            self.state.pending_non_call = false;
        }
        self.state.previous_ends_operand = matches!(
            kind,
            Kind::Name
                | Kind::Case
                | Kind::Int
                | Kind::String
                | Kind::FStringEnd
                | Kind::Rpar
                | Kind::Rsqb
                | Kind::Rbrace
        );
        None
    }
}

impl WidthFrame {
    fn exceeded(&self, limits: &Limits) -> Option<Reason> {
        let (limit, reason) = match self.kind {
            WidthKind::Call => (limits.max_call_arguments, Reason::CallArguments),
            WidthKind::Parameters => (limits.max_parameters, Reason::Parameters),
            WidthKind::Display if !self.comprehension => {
                (limits.max_display_items, Reason::DisplayItems)
            }
            WidthKind::Parenthesized if self.has_comma => {
                (limits.max_display_items, Reason::DisplayItems)
            }
            WidthKind::Slice if self.has_comma => (limits.max_slice_items, Reason::SliceItems),
            WidthKind::Display
            | WidthKind::Parenthesized
            | WidthKind::Slice
            | WidthKind::FString
            | WidthKind::Interpolation { .. }
            | WidthKind::Ignore => return None,
        };
        limit
            .is_some_and(|limit| self.items as usize > limit)
            .then_some(reason)
    }
}

// Track likely annotation positions rather than parse type expressions.
// Colons in slices, dictionaries, lambdas, and block headers need exclusions.
#[derive(Default)]
struct AnnotationTracker {
    brackets: Stack<AnnotationBracket, STRUCTURAL_CAPACITY>,
    state: AnnotationState,
}

#[derive(Default)]
struct AnnotationBracket {
    kind: Kind,
    // Value contexts propagate through nested calls, containers, and subscripts.
    value: bool,
    annotated_type: bool,
}

#[derive(Clone, Copy, Default)]
enum AnnotationName {
    #[default]
    Other,
    Literal,
    Annotated,
}

#[derive(Clone, Copy, Default)]
struct AnnotationState {
    // Separators end an annotation only at its starting bracket depth; commas
    // inside types such as tuple[int, str] remain part of the annotation.
    start_depth: Option<usize>,
    line_started: bool,
    block_header: bool,
    pending_lambdas: u32,
    previous: Option<(Kind, usize)>,
    previous_name: AnnotationName,
}

impl AnnotationTracker {
    #[cfg(feature = "experimental-regions")]
    fn region_ready(&self) -> bool {
        debug_assert!(self.brackets.is_empty() && !self.active());
        self.state.pending_lambdas == 0
    }

    fn reset(&mut self) {
        self.brackets.clear();
        self.state.start_depth = None;
        self.state.line_started = false;
        self.state.block_header = false;
        self.state.pending_lambdas = 0;
        self.state.previous = None;
        self.state.previous_name = AnnotationName::Other;
    }

    fn type_position(&self) -> bool {
        !self.brackets.last().is_some_and(|bracket| bracket.value)
    }

    fn active(&self) -> bool {
        self.state.start_depth.is_some()
    }

    fn observe(&mut self, source: &str, kind: Kind, range: &Range<usize>) {
        if matches!(kind, Kind::Newline | Kind::Semi) {
            self.state.start_depth = None;
            self.state.line_started = false;
            self.state.block_header = false;
            self.state.previous = None;
            self.state.previous_name = AnnotationName::Other;
            return;
        }
        if !self.state.line_started && !matches!(kind, Kind::Indent | Kind::Dedent) {
            self.state.line_started = true;
            self.state.block_header = matches!(
                &source[range.clone()],
                "if" | "elif"
                    | "else"
                    | "while"
                    | "for"
                    | "async"
                    | "with"
                    | "try"
                    | "except"
                    | "finally"
                    | "def"
                    | "class"
                    | "match"
                    | "case"
            );
        }
        if matches!(kind, Kind::Equal | Kind::Comma)
            && self.state.start_depth == Some(self.brackets.len())
        {
            self.state.start_depth = None;
        }
        if matches!(kind, Kind::Rpar | Kind::Rsqb | Kind::Rbrace)
            && self
                .state
                .start_depth
                .is_some_and(|depth| self.brackets.len() <= depth)
        {
            self.state.start_depth = None;
        }
        if kind == Kind::Lambda {
            self.state.pending_lambdas += 1;
        }
        if kind == Kind::Colon {
            if self.state.pending_lambdas > 0 {
                self.state.pending_lambdas -= 1;
            } else if self.type_position()
                && !matches!(
                    self.brackets.last().map(|bracket| bracket.kind),
                    Some(Kind::Lbrace | Kind::Lsqb)
                )
                && (!self.brackets.is_empty() || !self.state.block_header)
            {
                self.state.start_depth = Some(self.brackets.len());
            }
        }
        // The lexer emits `->` as two tokens. Require adjacent byte ranges so
        // separate minus/greater tokens cannot masquerade as a return annotation.
        if kind == Kind::Greater && self.state.previous == Some((Kind::Minus, range.start)) {
            self.state.start_depth = Some(self.brackets.len());
        }
        if kind == Kind::Comma
            && let Some(bracket) = self.brackets.last_mut()
            && bracket.annotated_type
        {
            // Only Annotated's first argument is a type. Its remaining operands
            // and all Literal operands are values, even inside quoted types.
            bracket.value = true;
            bracket.annotated_type = false;
        }
        match kind {
            Kind::Lpar | Kind::Lsqb | Kind::Lbrace => {
                let value = !self.type_position()
                    || (kind == Kind::Lsqb
                        && matches!(self.state.previous_name, AnnotationName::Literal));
                self.brackets.push(AnnotationBracket {
                    kind,
                    value,
                    annotated_type: kind == Kind::Lsqb
                        && matches!(self.state.previous_name, AnnotationName::Annotated),
                });
            }
            Kind::Rpar | Kind::Rsqb | Kind::Rbrace => {
                self.brackets.pop();
            }
            _ => {}
        }
        self.state.previous = Some((kind, range.end));
        self.state.previous_name = if kind == Kind::Name {
            match &source[range.clone()] {
                "Literal" => AnnotationName::Literal,
                "Annotated" => AnnotationName::Annotated,
                _ => AnnotationName::Other,
            }
        } else {
            AnnotationName::Other
        };
    }
}

impl Default for WidthFrame {
    fn default() -> Self {
        Self {
            kind: WidthKind::Ignore,
            items: 0,
            seen_item: false,
            has_comma: false,
            marker_only: false,
            comprehension: false,
            adjacent_fields: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str) -> Result<(), Rejection> {
        super::check(
            source,
            &Limits {
                max_source_bytes: 1_000_000,
                max_nesting_depth: 30,
                max_ast_depth: None,
                ..Limits::default()
            },
        )
    }

    #[test]
    fn fstrings_and_templates() {
        for source in [
            "f'{x}'",
            "rf'{x!r:>10}'",
            "t'{x}'",
            "f'{{literal}} {a[0]:{width}.{precision}f}'",
            "f\"{f'{x}'}\"",
            "f'{\"same quote\"}'",
            "f'{ {\"key\": value} }'",
            "f'{x:{{}}}'",
        ] {
            assert!(check(source).is_ok(), "{source}");
        }
        assert!(check(&format!("f'{}{{x}}{}'", "{x:".repeat(50), "}".repeat(50))).is_err());
        assert!(check(&format!("f'{{ {}1 }}'", "-".repeat(50))).is_err());
        assert!(check(&format!("f'{}'", "literal {{text}} ".repeat(1_000))).is_ok());
    }

    #[test]
    fn indentation_and_continuations() {
        let deep: String = (0..40)
            .map(|i| format!("{}if x:\n", " ".repeat(i)))
            .collect();
        assert!(check(&deep).is_err());
        assert!(check(&"if x:\n\tpass\nelse:\n\tpass\n".repeat(100)).is_ok());
        assert!(check(&format!("x = (\n{}1\n)\n", " ".repeat(1000))).is_ok());
        assert!(check(&format!("x = {}1", "-\\\n".repeat(50))).is_err());
        assert!(check("if x:\r\n\tpass\r\n# comment\r\nx = 1\r\n").is_ok());
    }

    #[test]
    fn quoted_annotations_and_bytes() {
        for quote in ["'", "\"", "'''", "\"\"\""] {
            assert!(
                check(&format!(
                    "x: {quote}{}int{}{quote}",
                    "[".repeat(50),
                    "]".repeat(50)
                ))
                .is_err()
            );
        }
        assert!(check(&format!("x = b'{}'", "(".repeat(100))).is_ok());
        assert!(check("x: \"list['int']\"").is_ok());
        assert!(check("x: \"\"\"'''\"'int'\"'''\"\"\"").is_ok());
    }

    #[test]
    fn malformed_and_unicode() {
        for source in [
            "π = '🦀'",
            "'unterminated\\",
            "\"\"\"",
            "\\\n",
            "\0🦀",
            "f'{",
            "([)]",
        ] {
            let mut scratch = LexerScratch::default();
            for token in Lexer::new(source, 30, 30, &mut scratch) {
                assert!(source.is_char_boundary(token.range.start));
                assert!(source.is_char_boundary(token.range.end));
            }
        }
        // An unterminated short string cannot hide code on following lines.
        assert!(check(&format!("'broken\n{}1", "-".repeat(50))).is_err());
    }

    #[test]
    fn seeded_malformed_sources_terminate() {
        let alphabet = [
            'a', 'π', '🦀', '0', 'f', 'r', '\'', '"', '{', '}', '(', ')', '[', ']', ':', '\\',
            '\n', '\r', '\t', '#', '-', '*',
        ];
        let mut state = 42u64;
        for _ in 0..2_000 {
            let mut source = String::new();
            for _ in 0..100 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                source.push(alphabet[(state >> 32) as usize % alphabet.len()]);
            }
            let mut scratch = LexerScratch::default();
            let tokens: Vec<_> = Lexer::new(&source, 30, 30, &mut scratch)
                .take(source.len() * 2 + 32)
                .collect();
            assert!(tokens.len() < source.len() * 2 + 32);
            for token in tokens {
                assert!(source.is_char_boundary(token.range.start));
                assert!(source.is_char_boundary(token.range.end));
            }
            let _ = check(&source);
        }
    }

    #[test]
    fn ordinary_flat_code() {
        assert!(check(&"x = [-1, -2]\n".repeat(100)).is_ok());
        assert!(check(&format!("x = [{}]", "-1, ".repeat(3000))).is_ok());
        assert!(check(&format!("x = {{{}}}", "'k': lambda a, b: 0, ".repeat(500))).is_ok());
    }
}

#[cfg(test)]
mod fast_path_tests {
    use super::*;

    #[test]
    fn simple_prefix_matches_detailed_scanning() {
        let lines = [
            "x = 1\n",
            "value = left.attr + right * 2\r\n",
            "x = ~~--1 ** 2\n",
            "x != y <= z >> 2 // 3\n",
            "async await yield not in is and or if for\n",
            "case, x, 1.2e3, 0xdead\n",
            "else x\n",
            "elif x\n",
            "lambda x\n",
            "x = '\n",
            "x: '[[[[int]]]]'\n",
            "x = f'{y:{z}}'\n",
            "(x\n + y)\n",
            "x = 1 + \\\n2\n",
            "π = '🦀'\n",
            "\t\x0cx = 1\n",
            "if x:\n pass\n",
            "# comment\n",
            "x = 1\r",
            "x = 1",
            "",
        ];
        let mut random = 123u64;

        for depth in [0, 1, 4, 30, 100] {
            for tokens in [0, 1, 4, 20, 1000] {
                for token_bytes in [None, Some(0), Some(1), Some(8)] {
                    let limits = Limits {
                        max_nesting_depth: depth,
                        max_ast_depth: Some(depth / 2),
                        max_tokens: tokens,
                        max_token_bytes: token_bytes,
                        max_annotation_depth: 3,
                        ..Limits::default()
                    };
                    let mut checker = Checker::new(limits.clone()).expect("valid limits");

                    for _ in 0..100 {
                        let mut source = String::new();
                        for _ in 0..10 {
                            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                            source.push_str(lines[(random >> 32) as usize % lines.len()]);
                        }
                        let actual = checker.check(&source);
                        let expected = scan::<false, false>(
                            &source,
                            &limits,
                            &mut checker.scratch,
                            &mut checker.work,
                        );
                        assert_eq!(actual, expected, "{source:?}");
                    }
                }
            }
        }
    }
}
