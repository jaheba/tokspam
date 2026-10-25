//! Pre-parse screening with an independent lexer.
//! Acceptance is not yet a verified bound on Python parser recursion.
//!
//! ```
//! use tokspam::{Limits, check};
//! let limits = Limits { max_nesting_depth: 80, ..Limits::default() };
//! assert!(check("x = [1, 2, 3]", &limits).is_ok());
//! ```
// Keep the public surface here; implementation modules follow the scan pipeline.
mod checker;
mod error;
mod lexer;
mod limits;
mod rules;
mod stack;

pub use checker::{Checker, check};
pub use error::{Reason, Rejection};
pub use limits::{
    ConfigError, LimitKind, Limits, MAX_ANNOTATION_DEPTH, MAX_AST_DEPTH, MAX_CONSTRUCT_WIDTH,
    MAX_INDENTATION_DEPTH, MAX_NESTING_DEPTH, MAX_SOURCE_BYTES, MAX_TOKEN_BYTES, MAX_TOKENS,
    MAX_WORK_UNITS,
};
