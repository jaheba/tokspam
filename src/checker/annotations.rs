//! Locate likely type strings and distinguish type operands from metadata values.

use crate::Reason;
use crate::lexer::Kind;
use crate::stack::Stack;
use std::ops::Range;

// Track likely annotation positions rather than parse type expressions.
// Colons in slices, dictionaries, lambdas, and block headers need exclusions.
#[derive(Default)]
pub(super) struct AnnotationTracker {
    pub(super) brackets: Stack<AnnotationBracket>,
    pub(super) state: AnnotationState,
}

#[derive(Default)]
pub(super) struct AnnotationBracket {
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
pub(super) struct AnnotationState {
    // Separators end an annotation only at its starting bracket depth; commas
    // inside types such as tuple[int, str] remain part of the annotation.
    start_depth: Option<usize>,
    line_started: bool,
    block_header: bool,
    pending_lambdas: usize,
    previous: Option<(Kind, usize)>,
    previous_name: AnnotationName,
}

impl AnnotationTracker {
    #[cfg(feature = "regions")]
    #[inline]
    pub(super) fn region_ready(&self) -> bool {
        debug_assert!(self.brackets.is_empty() && !self.active());
        self.state.pending_lambdas == 0
    }

    #[inline]
    pub(super) fn reset(&mut self) {
        self.brackets.clear();
        self.state.start_depth = None;
        self.state.line_started = false;
        self.state.block_header = false;
        self.state.pending_lambdas = 0;
        self.state.previous = None;
        self.state.previous_name = AnnotationName::Other;
    }

    #[inline]
    pub(super) fn type_position(&self) -> bool {
        !self.brackets.last().is_some_and(|bracket| bracket.value)
    }

    #[inline]
    pub(super) fn active(&self) -> bool {
        self.state.start_depth.is_some()
    }

    // This runs for every token. Preserve specialization at the caller across
    // the module boundary, especially the common non-annotation path.
    #[inline(always)]
    pub(super) fn observe(
        &mut self,
        source: &str,
        kind: Kind,
        range: &Range<usize>,
    ) -> Option<Reason> {
        if matches!(kind, Kind::Newline | Kind::Semi) {
            self.state.start_depth = None;
            self.state.line_started = false;
            self.state.block_header = false;
            self.state.previous = None;
            self.state.previous_name = AnnotationName::Other;
            return None;
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
                if !self.brackets.push(AnnotationBracket {
                    kind,
                    value,
                    annotated_type: kind == Kind::Lsqb
                        && matches!(self.state.previous_name, AnnotationName::Annotated),
                }) {
                    return Some(Reason::Storage);
                }
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
        None
    }
}
