//! Syntactic width limits and capacity-only tracking when those limits are disabled.

use crate::lexer::Kind;
use crate::stack::{STRUCTURAL_CAPACITY, Stack};
use crate::{Limits, MAX_TOKENS, Reason};
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

const _: () = assert!(MAX_TOKENS <= u32::MAX as usize);

pub(super) struct WidthFrame {
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
    pub(super) frames: Stack<WidthFrame, STRUCTURAL_CAPACITY>,
    pub(super) state: WidthState,
}

#[derive(Clone, Copy, Default)]
pub(super) struct WidthState {
    previous_ends_operand: bool,
    pending_def: bool,
    pending_non_call: bool,
    root_adjacent_fields: u32,
}

impl WidthTracker {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_storage(true)
    }

    #[inline]
    pub(super) fn with_storage(preallocated: bool) -> Self {
        Self {
            frames: Stack::with_storage(STRUCTURAL_CAPACITY, preallocated),
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

    // With all width limits disabled, item counts and display classifications
    // cannot reject. Preserve only the same frame openings/closures and hard
    // capacity guard, including unfinished lambda parameters on malformed input.
    #[inline]
    pub(super) fn observe_capacity(
        &mut self,
        source: &str,
        kind: Kind,
        range: &Range<usize>,
    ) -> Option<Reason> {
        if kind == Kind::Name && &source[range.clone()] == "def" {
            self.state.pending_def = true;
        }
        let parameters = kind == Kind::Lambda || (kind == Kind::Lpar && self.state.pending_def);
        if matches!(
            kind,
            Kind::Lambda | Kind::Lpar | Kind::Lsqb | Kind::Lbrace | Kind::FStringStart
        ) {
            if kind == Kind::Lpar {
                self.state.pending_def = false;
            }
            if self.frames.is_full() {
                return Some(Reason::Inconclusive);
            }
            self.frames.push(WidthFrame {
                kind: if parameters {
                    WidthKind::Parameters
                } else {
                    WidthKind::Ignore
                },
                items: 0,
                seen_item: false,
                has_comma: false,
                marker_only: false,
                comprehension: false,
                adjacent_fields: 0,
            });
        } else if matches!(
            kind,
            Kind::Rpar | Kind::Rsqb | Kind::Rbrace | Kind::FStringEnd
        ) || (kind == Kind::Colon
            && self
                .frames
                .last()
                .is_some_and(|frame| matches!(frame.kind, WidthKind::Parameters)))
        {
            self.frames.pop();
        }
        if matches!(kind, Kind::Newline | Kind::Semi | Kind::Colon) {
            self.state.pending_def = false;
        }
        None
    }

    #[inline]
    pub(super) fn observe(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(
        full: &mut WidthTracker,
        capacity: &mut WidthTracker,
        kind: Kind,
        text: &str,
    ) -> bool {
        let range = 0..text.len();
        let expected = full.observe(text, kind, &range, &Limits::default());
        let actual = capacity.observe_capacity(text, kind, &range);
        assert_eq!(actual, expected, "{kind:?} {text:?}");
        assert_eq!(capacity.frames.len(), full.frames.len());
        assert_eq!(capacity.state.pending_def, full.state.pending_def);
        assert!(
            capacity
                .frames
                .iter()
                .zip(full.frames.iter())
                .all(|(a, b)| {
                    matches!(a.kind, WidthKind::Parameters)
                        == matches!(b.kind, WidthKind::Parameters)
                })
        );
        expected.is_some()
    }

    #[test]
    fn capacity_only_tracking_matches_full_tracking_without_width_limits() {
        let alphabet = [
            (Kind::Name, "def"),
            (Kind::Name, "class"),
            (Kind::Name, "import"),
            (Kind::Name, "x"),
            (Kind::Name, "π"),
            (Kind::Lambda, "lambda"),
            (Kind::Lpar, "("),
            (Kind::Lsqb, "["),
            (Kind::Lbrace, "{"),
            (Kind::Rpar, ")"),
            (Kind::Rsqb, "]"),
            (Kind::Rbrace, "}"),
            (Kind::Colon, ":"),
            (Kind::Comma, ","),
            (Kind::Equal, "="),
            (Kind::Newline, "\n"),
            (Kind::Semi, ";"),
            (Kind::Star, "*"),
            (Kind::Slash, "/"),
            (Kind::For, "for"),
            (Kind::String, "'x'"),
            (Kind::FStringStart, "f'"),
            (Kind::FStringEnd, "'"),
        ];
        let mut random = 42u64;
        let mut full = WidthTracker::new();
        let mut capacity = WidthTracker::new();
        for _ in 0..1000 {
            full.frames.reset();
            capacity.frames.reset();
            full.reset();
            capacity.reset();
            for _ in 0..256 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let (kind, text) = alphabet[(random >> 32) as usize % alphabet.len()];
                if compare(&mut full, &mut capacity, kind, text) {
                    break;
                }
            }
        }
    }

    #[test]
    fn capacity_only_tracking_preserves_shared_annotation_capacity() {
        let mut full = WidthTracker::new();
        let mut capacity = WidthTracker::new();
        for _ in 0..STRUCTURAL_CAPACITY - 2 {
            assert!(!compare(&mut full, &mut capacity, Kind::Lambda, "lambda"));
        }
        let full_state = full.state;
        let capacity_state = capacity.state;
        let full_mark = full.frames.suspend();
        let capacity_mark = capacity.frames.suspend();
        full.reset();
        capacity.reset();
        assert!(!compare(&mut full, &mut capacity, Kind::Lpar, "("));
        assert!(!compare(&mut full, &mut capacity, Kind::Lambda, "lambda"));
        assert!(compare(&mut full, &mut capacity, Kind::Lpar, "("));
        full.frames.restore(full_mark);
        capacity.frames.restore(capacity_mark);
        full.state = full_state;
        capacity.state = capacity_state;
        assert!(!compare(&mut full, &mut capacity, Kind::Colon, ":"));
        assert!(!compare(&mut full, &mut capacity, Kind::Lpar, "("));
    }
}
