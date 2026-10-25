//! Screening outcomes and source locations returned to callers.

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
    /// Screening exhausted its work budget or bounded tracking capacity before
    /// establishing a structural violation.
    Inconclusive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub reason: Reason,
    /// Byte range in the original source; nested type failures identify the
    /// enclosing quoted annotation rather than a range in its sliced contents.
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
