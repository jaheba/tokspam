//! Streaming lexer retaining source ranges without decoding literals.
//!
//! `code` drives token dispatch; `strings` and `indentation` handle their state
//! machines. `bytes` supplies shared search/classification primitives, while
//! `simple_lines` and the optional `region` module prove conservative shortcuts.
//! All paths share the same cursor and bounded segmented scratch storage.

use crate::stack::{Mark, STRUCTURAL_CAPACITY, Stack};

mod bytes;
mod code;
mod indentation;
#[cfg(feature = "regions")]
mod region;
mod simple_lines;
mod strings;
#[cfg(test)]
mod tests;
mod token;

use bytes::{BYTE_CLASSES, NAME_BYTE, NUMBER_BYTE};
pub use token::{Kind, Token};

// A tab contributes at most eight columns per source byte; indentation storage
// uses u32 columns under this hard source-size ceiling.
const _: () = assert!(crate::MAX_SOURCE_BYTES <= u32::MAX as usize / 8);

#[derive(Clone, Copy, Default)]
enum Context {
    // Only root code emits logical newlines and indentation tokens.
    #[default]
    Root,

    // Retain the expected closer so mismatches cannot discard an outer mode.
    Bracket(u8),

    // Literal text in an f/t-string; braces temporarily switch back to code.
    Text {
        quote: u8,
        // Python quote delimiters are one or three bytes wide.
        width: u8,
    },

    // Replacement-field expression. A colon replaces this mode with Format.
    Field,

    // Format text may open nested replacement fields before its closing brace.
    Format,
}

// Shared segmented storage: suspended annotations keep their entries below
// the active base. Marks are indices, so growing Vecs may relocate safely.
#[derive(Default)]
pub(super) struct LexerScratch {
    contexts: Stack<Context, STRUCTURAL_CAPACITY>,
    indents: Stack<u32, STRUCTURAL_CAPACITY>,
}

// Save only cursor state when an annotation suspends this lexer. The caller
// retains context/indent entries in scratch instead of copying either stack.
#[derive(Clone, Copy)]
pub(super) struct LexerState {
    position: usize,
    line_start: bool,
    // One physical line can emit several Dedents; retain its target column
    // until the iterator has returned them all, without rescanning whitespace.
    pending_indent: Option<usize>,
    stopped: bool,
}

impl LexerScratch {
    pub(super) fn with_storage(contexts: usize, indents: usize, preallocated: bool) -> Self {
        Self {
            contexts: Stack::with_storage(contexts, preallocated),
            indents: Stack::with_storage(indents, preallocated),
        }
    }

    pub(super) fn suspend(&mut self) -> [Mark; 2] {
        [self.contexts.suspend(), self.indents.suspend()]
    }

    pub(super) fn restore(&mut self, saved: [Mark; 2]) {
        self.contexts.restore(saved[0]);
        self.indents.restore(saved[1]);
    }

    pub(super) fn clear_segments(&mut self) {
        self.contexts.reset();
        self.indents.reset();
    }
}

pub struct Lexer<'a, 's> {
    source: &'a str,
    position: usize,
    contexts: &'s mut Stack<Context, STRUCTURAL_CAPACITY>,
    indents: &'s mut Stack<u32, STRUCTURAL_CAPACITY>,
    line_start: bool,
    pending_indent: Option<usize>,
    limit: usize,
    indentation_limit: usize,
    stopped: bool,
}

impl<'a, 's> Lexer<'a, 's> {
    pub fn new(
        source: &'a str,
        limit: usize,
        indentation_limit: usize,
        scratch: &'s mut LexerScratch,
    ) -> Self {
        // Clear only the active segment; a suspended parent's entries remain
        // intact below its base when this starts an annotation child scan.
        scratch.contexts.clear();
        scratch.contexts.push(Context::Root);
        scratch.indents.clear();
        scratch.indents.push(0);

        Self {
            source,
            position: 0,
            contexts: &mut scratch.contexts,
            indents: &mut scratch.indents,
            line_start: true,
            pending_indent: None,
            limit,
            indentation_limit,
            stopped: false,
        }
    }

    pub(super) fn resume(
        source: &'a str,
        limit: usize,
        indentation_limit: usize,
        scratch: &'s mut LexerScratch,
        state: LexerState,
    ) -> Self {
        Self {
            source,
            position: state.position,
            contexts: &mut scratch.contexts,
            indents: &mut scratch.indents,
            line_start: state.line_start,
            pending_indent: state.pending_indent,
            limit,
            indentation_limit,
            stopped: state.stopped,
        }
    }

    pub(super) fn state(&self) -> LexerState {
        LexerState {
            position: self.position,
            line_start: self.line_start,
            pending_indent: self.pending_indent,
            stopped: self.stopped,
        }
    }

    fn token(&self, kind: Kind, start: usize) -> Token {
        Token {
            kind,
            range: start..self.position,
            contents: None,
        }
    }

    #[cold]
    fn invalid_code_byte(&mut self, start: usize) -> Token {
        self.stopped = true;
        self.token(Kind::Limit, start)
    }

    fn push(&mut self, context: Context) -> bool {
        if self.contexts.len() > self.limit {
            self.stopped = true;
            false
        } else {
            self.contexts.push(context);
            true
        }
    }
}
