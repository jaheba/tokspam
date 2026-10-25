//! Syntactic width limits and capacity-only tracking when those limits are disabled.

use crate::lexer::Kind;
use crate::stack::Stack;
use crate::{Limits, Reason, Scores};
use std::ops::Range;

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

pub(super) struct WidthFrame {
    kind: WidthKind,
    // Use full-size counters: optional score thresholds impose no small ceiling.
    items: usize,
    // Commas finalize items; the closing delimiter charges a nonempty final
    // item. This avoids counting empty constructs or trailing commas twice.
    seen_item: bool,
    has_comma: bool,
    marker_only: bool,
    comprehension: bool,
    adjacent_fields: usize,
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

// Classify delimiters lexically so nested contents count as one parent item.
// This estimates construct widths without validating Python's full grammar.
pub(super) struct WidthTracker {
    pub(super) frames: Stack<WidthFrame>,
    pub(super) state: WidthState,
}

#[derive(Clone, Copy, Default)]
pub(super) struct WidthState {
    previous_ends_operand: bool,
    pending_def: bool,
    pending_non_call: bool,
    root_adjacent_fields: usize,
}

impl WidthTracker {
    #[inline]
    pub(super) fn with_storage(capacity: usize, preallocated: bool) -> Self {
        Self {
            frames: Stack::with_storage(capacity, preallocated),
            state: WidthState::default(),
        }
    }

    #[inline]
    pub(super) fn reset(&mut self) {
        self.frames.clear();
        self.state.previous_ends_operand = false;
        self.state.pending_def = false;
        self.state.pending_non_call = false;
        self.state.root_adjacent_fields = 0;
    }

    #[inline]
    pub(super) fn observe<const COLLECT: bool>(
        &mut self,
        source: &str,
        kind: Kind,
        range: &Range<usize>,
        limits: &Limits,
        scores: &mut Scores,
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
                    if COLLECT {
                        scores.interpolation_fields = scores.interpolation_fields.max(parent.items);
                    }
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
            if !self.frames.push(WidthFrame {
                kind: opened,
                items: adjacent_fields,
                seen_item: false,
                has_comma: false,
                marker_only: false,
                comprehension: false,
                adjacent_fields: 0,
            }) {
                return Some(Reason::Storage);
            }
        } else if kind == Kind::Comma {
            if let Some(frame) = self.frames.last_mut() {
                frame.has_comma = true;
                if frame.seen_item && !frame.marker_only {
                    frame.items += 1;
                }
                frame.seen_item = false;
                frame.marker_only = false;
                if COLLECT {
                    frame.record(scores);
                }
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
                if COLLECT {
                    frame.record(scores);
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
    fn record(&self, scores: &mut Scores) {
        let score = match self.kind {
            WidthKind::Call => &mut scores.call_arguments,
            WidthKind::Parameters => &mut scores.parameters,
            WidthKind::Display if !self.comprehension => &mut scores.display_items,
            WidthKind::Parenthesized if self.has_comma => &mut scores.display_items,
            WidthKind::Slice if self.has_comma => &mut scores.slice_items,
            WidthKind::FString | WidthKind::Interpolation { .. } => {
                &mut scores.interpolation_fields
            }
            _ => return,
        };
        *score = (*score).max(self.items);
    }
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
