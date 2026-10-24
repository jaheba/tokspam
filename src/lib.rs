//! Experimental pre-parse screening with an independent lexer.
//! Acceptance is not yet a verified bound on Python parser recursion.
//!
//! ```
//! use spam::{Limits, check};
//! let limits = Limits { max_nesting_depth: 80, ..Limits::default() };
//! assert!(check("x = [1, 2, 3]", &limits).is_ok());
//! ```
mod lexer;
mod rules;

use lexer::{Kind, Lexer, LexerScratch, LexerState};
use rules::Scanner;
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
    #[cfg(feature = "experimental-width-limits")]
    CallArguments,
    #[cfg(feature = "experimental-width-limits")]
    Parameters,
    #[cfg(feature = "experimental-width-limits")]
    DisplayItems,
    #[cfg(feature = "experimental-width-limits")]
    SliceItems,
    #[cfg(feature = "experimental-width-limits")]
    InterpolationFields,
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
pub const MAX_TOKEN_BYTES: usize = MAX_SOURCE_BYTES;
pub const MAX_NESTING_DEPTH: u16 = 1_024;
pub const MAX_INDENTATION_DEPTH: u16 = 1_024;
pub const MAX_ANNOTATION_DEPTH: u8 = 32;
pub const MAX_AST_DEPTH: u16 = 4_096;
#[cfg(feature = "experimental-width-limits")]
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
    #[cfg(feature = "experimental-width-limits")]
    CallArguments,
    #[cfg(feature = "experimental-width-limits")]
    Parameters,
    #[cfg(feature = "experimental-width-limits")]
    DisplayItems,
    #[cfg(feature = "experimental-width-limits")]
    SliceItems,
    #[cfg(feature = "experimental-width-limits")]
    InterpolationFields,
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
    pub max_token_bytes: Option<usize>,
    pub max_nesting_depth: u16,
    pub max_indentation_depth: u16,
    pub max_annotation_depth: u8,
    pub check_string_annotations: bool,
    /// Flat AST-shape estimates supplement the parser-recursion rules.
    pub max_ast_depth: Option<u16>,
    /// Experimental flat-width budgets. `None` preserves the normal scanner path.
    #[cfg(feature = "experimental-width-limits")]
    pub max_call_arguments: Option<usize>,
    #[cfg(feature = "experimental-width-limits")]
    pub max_parameters: Option<usize>,
    #[cfg(feature = "experimental-width-limits")]
    pub max_display_items: Option<usize>,
    #[cfg(feature = "experimental-width-limits")]
    pub max_slice_items: Option<usize>,
    #[cfg(feature = "experimental-width-limits")]
    pub max_interpolation_fields: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            max_tokens: 100_000,
            max_token_bytes: None,
            max_nesting_depth: 100,
            max_indentation_depth: 80,
            max_annotation_depth: 8,
            check_string_annotations: true,
            max_ast_depth: Some(100),
            #[cfg(feature = "experimental-width-limits")]
            max_call_arguments: None,
            #[cfg(feature = "experimental-width-limits")]
            max_parameters: None,
            #[cfg(feature = "experimental-width-limits")]
            max_display_items: None,
            #[cfg(feature = "experimental-width-limits")]
            max_slice_items: None,
            #[cfg(feature = "experimental-width-limits")]
            max_interpolation_fields: None,
        }
    }
}

impl Limits {
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_limit(
            self.max_source_bytes,
            MAX_SOURCE_BYTES,
            LimitKind::SourceBytes,
        )?;
        validate_limit(self.max_tokens, MAX_TOKENS, LimitKind::Tokens)?;
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
        #[cfg(feature = "experimental-width-limits")]
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
/// Scratch allocations depend only on `limits` and finish before token scanning.
/// Oversized source is rejected before allocating scratch memory.
/// Use [`Checker`] to amortize scratch allocation across multiple inputs.
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

/// Reusable screening state. Construction allocates scratch storage from the
/// configured limits; subsequent checks perform no heap allocations.
///
/// Use one checker per worker when screening many inputs before parsing them.
/// Source strings are borrowed only for the duration of each check.
pub struct Checker {
    limits: Limits,
    scratch: Vec<ScanScratch>,
    work: Vec<ScanWork>,
}

impl Checker {
    pub fn new(limits: Limits) -> Result<Self, ConfigError> {
        limits.validate()?;
        Ok(Self::new_validated(limits))
    }

    fn new_validated(limits: Limits) -> Self {
        let annotation_depth = if limits.check_string_annotations {
            limits.max_annotation_depth
        } else {
            0
        };
        let scratch = (0..=annotation_depth)
            .map(|_| ScanScratch::new(&limits))
            .collect();

        let work = Vec::with_capacity(usize::from(annotation_depth) + 1);

        Self {
            limits,
            scratch,
            work,
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

struct ScanWork {
    source: Range<usize>,
    budget: u32,
    ast_budget: Option<u32>,
    annotation_source: bool,
    depth: u8,
    lexer: Option<LexerState>,
    region_start: bool,
    outer_annotation: Option<Range<usize>>,
}

// Allocate every annotation frame before inspecting tokens. Reusing these frames
// keeps both allocation count and capacity independent of the source structure.
struct ScanScratch {
    lexer: LexerScratch,
    scanner: Scanner,
    annotations: AnnotationTracker,
    #[cfg(feature = "experimental-width-limits")]
    widths: WidthTracker,
}

impl ScanScratch {
    fn new(limits: &Limits) -> Self {
        let budget = u32::from(limits.max_nesting_depth);

        Self {
            lexer: LexerScratch::new(budget as usize, usize::from(limits.max_indentation_depth)),
            scanner: Scanner::new(budget, limits.max_ast_depth.map(u32::from)),
            annotations: AnnotationTracker {
                brackets: Vec::with_capacity(budget as usize),
                ..AnnotationTracker::default()
            },
            #[cfg(feature = "experimental-width-limits")]
            widths: WidthTracker::new(budget as usize),
        }
    }
}

fn scan<const FAST: bool, const REGIONS: bool>(
    source: &str,
    limits: &Limits,
    scratch: &mut [ScanScratch],
    work: &mut Vec<ScanWork>,
) -> Result<(), Rejection> {
    work.clear();
    work.push(ScanWork {
        source: 0..source.len(),
        budget: u32::from(limits.max_nesting_depth),
        ast_budget: limits.max_ast_depth.map(u32::from),
        annotation_source: false,
        depth: 0,
        lexer: None,
        region_start: true,
        outer_annotation: None,
    });
    let mut tokens_used = 0usize;
    let mut bytes_scanned = source.len();

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
            #[cfg(feature = "experimental-width-limits")]
            widths,
        } = &mut scratch[index];

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
            #[cfg(feature = "experimental-width-limits")]
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
                    reason: if current.outer_annotation.is_some() {
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
            #[cfg(feature = "experimental-width-limits")]
            if let Some(reason) = widths.observe(frame_source, token.kind, &token.range, limits) {
                return reject(reason, absolute(&token.range));
            }

            let mut child = None;
            if limits.check_string_annotations
                && (current.annotation_source || annotations.active())
            {
                if let Some(contents) = token.contents.clone() {
                    let remaining = current.budget.saturating_sub(scanner.total);
                    let ast_remaining = current
                        .ast_budget
                        .map(|budget| budget.saturating_sub(scanner.ast_total));
                    let minimum = remaining.min(ast_remaining.unwrap_or(remaining));
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
                        child = Some(ScanWork {
                            source: absolute(&contents),
                            budget: remaining,
                            ast_budget: ast_remaining,
                            annotation_source: true,
                            depth: current.depth + 1,
                            lexer: None,
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
                current.lexer = Some(lexer.state());
                break Some(child);
            }
        };

        if let Some(child) = next_child {
            work.push(child);
        } else {
            work.pop();
        }
    }
}

#[cfg(feature = "experimental-regions")]
fn widths_enabled(limits: &Limits) -> bool {
    #[cfg(feature = "experimental-width-limits")]
    {
        limits.widths_enabled()
    }
    #[cfg(not(feature = "experimental-width-limits"))]
    {
        let _ = limits;
        false
    }
}

#[cfg(all(
    feature = "experimental-regions",
    feature = "experimental-width-limits"
))]
impl Limits {
    fn widths_enabled(&self) -> bool {
        self.max_call_arguments.is_some()
            || self.max_parameters.is_some()
            || self.max_display_items.is_some()
            || self.max_slice_items.is_some()
            || self.max_interpolation_fields.is_some()
    }
}

#[cfg(feature = "experimental-width-limits")]
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

#[cfg(feature = "experimental-width-limits")]
struct WidthFrame {
    kind: WidthKind,
    items: usize,
    seen_item: bool,
    has_comma: bool,
    marker_only: bool,
    comprehension: bool,
    adjacent_fields: usize,
}

#[cfg(feature = "experimental-width-limits")]
struct WidthTracker {
    frames: Vec<WidthFrame>,
    previous_ends_operand: bool,
    pending_def: bool,
    pending_non_call: bool,
    root_adjacent_fields: usize,
}

#[cfg(feature = "experimental-width-limits")]
impl WidthTracker {
    fn new(capacity: usize) -> Self {
        Self {
            frames: Vec::with_capacity(capacity + 1),
            previous_ends_operand: false,
            pending_def: false,
            pending_non_call: false,
            root_adjacent_fields: 0,
        }
    }

    fn reset(&mut self) {
        self.frames.clear();
        self.previous_ends_operand = false;
        self.pending_def = false;
        self.pending_non_call = false;
        self.root_adjacent_fields = 0;
    }

    fn observe(
        &mut self,
        source: &str,
        kind: Kind,
        range: &Range<usize>,
        limits: &Limits,
    ) -> Option<Reason> {
        if kind == Kind::Name && &source[range.clone()] == "def" {
            self.pending_def = true;
        }
        if kind == Kind::Name && matches!(&source[range.clone()], "class" | "import") {
            self.pending_non_call = true;
        }

        let opened = match kind {
            Kind::FStringStart => Some(WidthKind::FString),
            Kind::Lambda => Some(WidthKind::Parameters),
            Kind::Lpar if self.pending_def => {
                self.pending_def = false;
                Some(WidthKind::Parameters)
            }
            Kind::Lpar if self.pending_non_call => {
                self.pending_non_call = false;
                Some(WidthKind::Ignore)
            }
            Kind::Lpar if self.previous_ends_operand => Some(WidthKind::Call),
            Kind::Lpar => Some(WidthKind::Parenthesized),
            Kind::Lsqb if self.previous_ends_operand => Some(WidthKind::Slice),
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
                        .is_some_and(|limit| parent.items > limit)
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
            let adjacent_fields = if matches!(opened, WidthKind::FString) {
                self.frames
                    .last()
                    .map_or(self.root_adjacent_fields, |frame| frame.adjacent_fields)
            } else {
                if let Some(parent) = self.frames.last_mut() {
                    parent.adjacent_fields = 0;
                } else {
                    self.root_adjacent_fields = 0;
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
                    frame.items += usize::from(frame.seen_item && !frame.marker_only);
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
                    self.root_adjacent_fields = adjacent_fields;
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
                    self.root_adjacent_fields = 0;
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
                        frame.marker_only = true;
                    } else if frame.seen_item {
                        frame.marker_only = false;
                    }
                    frame.seen_item = true;
                }
            }
        }

        if matches!(kind, Kind::Newline | Kind::Semi | Kind::Colon) {
            self.pending_def = false;
            self.pending_non_call = false;
        }
        self.previous_ends_operand = matches!(
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

#[cfg(feature = "experimental-width-limits")]
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
            .is_some_and(|limit| self.items > limit)
            .then_some(reason)
    }
}

#[derive(Default)]
struct AnnotationTracker {
    brackets: Vec<Kind>,
    start_depth: Option<usize>,
    line_started: bool,
    block_header: bool,
    pending_lambdas: u32,
    previous: Option<(Kind, usize)>,
}

impl AnnotationTracker {
    #[cfg(feature = "experimental-regions")]
    fn region_ready(&self) -> bool {
        debug_assert!(self.brackets.is_empty() && !self.active());
        self.pending_lambdas == 0
    }

    fn reset(&mut self) {
        self.brackets.clear();
        self.start_depth = None;
        self.line_started = false;
        self.block_header = false;
        self.pending_lambdas = 0;
        self.previous = None;
    }

    fn active(&self) -> bool {
        self.start_depth.is_some()
    }

    fn observe(&mut self, source: &str, kind: Kind, range: &Range<usize>) {
        if matches!(kind, Kind::Newline | Kind::Semi) {
            self.start_depth = None;
            self.line_started = false;
            self.block_header = false;
            self.previous = None;
            return;
        }
        if !self.line_started && !matches!(kind, Kind::Indent | Kind::Dedent) {
            self.line_started = true;
            self.block_header = matches!(
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
            && self.start_depth == Some(self.brackets.len())
        {
            self.start_depth = None;
        }
        if matches!(kind, Kind::Rpar | Kind::Rsqb | Kind::Rbrace)
            && self
                .start_depth
                .is_some_and(|depth| self.brackets.len() <= depth)
        {
            self.start_depth = None;
        }
        if kind == Kind::Lambda {
            self.pending_lambdas += 1;
        }
        if kind == Kind::Colon {
            if self.pending_lambdas > 0 {
                self.pending_lambdas -= 1;
            } else if !matches!(self.brackets.last(), Some(Kind::Lbrace | Kind::Lsqb))
                && (!self.brackets.is_empty() || !self.block_header)
            {
                self.start_depth = Some(self.brackets.len());
            }
        }
        if kind == Kind::Greater && self.previous == Some((Kind::Minus, range.start)) {
            self.start_depth = Some(self.brackets.len());
        }
        match kind {
            Kind::Lpar | Kind::Lsqb | Kind::Lbrace => self.brackets.push(kind),
            Kind::Rpar | Kind::Rsqb | Kind::Rbrace => {
                self.brackets.pop();
            }
            _ => {}
        }
        self.previous = Some((kind, range.end));
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
            let mut scratch = LexerScratch::new(30, 30);
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
            let mut scratch = LexerScratch::new(30, 30);
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
