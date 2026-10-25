//! Reduced token vocabulary for structural accounting, not Python syntax validation.

use std::ops::Range;

// Only distinctions needed by the resource rules get dedicated kinds. Other
// keywords, including def/class, remain Name and are inspected by range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Name,
    Int,
    String,
    FStringStart,
    FStringEnd,
    Lpar,
    Lsqb,
    Lbrace,
    Rpar,
    Rsqb,
    Rbrace,
    Indent,
    Dedent,
    Newline,
    Semi,
    Minus,
    Plus,
    Star,
    Not,
    Tilde,
    Await,
    Yield,
    Async,
    DoubleStar,
    Lambda,
    Match,
    Case,
    Colon,
    Else,
    Comma,
    Slash,
    DoubleSlash,
    Percent,
    At,
    Vbar,
    Amper,
    CircumFlex,
    LeftShift,
    RightShift,
    Less,
    Greater,
    EqEqual,
    NotEqual,
    LessEqual,
    GreaterEqual,
    In,
    Is,
    And,
    Or,
    If,
    Elif,
    For,
    Dot,
    Equal,
    Other,
    Limit,
    StorageLimit,
    NestingLimit,
    IndentationLimit,
}

#[derive(Debug)]
pub struct Token {
    pub kind: Kind,
    // UTF-8-aligned byte offsets; synthetic layout/EOF tokens may be empty.
    pub range: Range<usize>,
    // Undecoded contents of a non-byte plain string for annotation screening.
    // Interpolated strings instead emit their expression tokens directly.
    pub contents: Option<Range<usize>>,
}
