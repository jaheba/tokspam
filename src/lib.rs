//! Python source complexity scores and optional early-exit limits.
//! Acceptance is not yet a verified bound on Python parser recursion.
//!
//! ```
//! use tokspam::{Limits, check};
//! let limits = Limits { max_nesting_depth: Some(80), ..Limits::default() };
//! assert!(check("x = [1, 2, 3]", &limits).is_ok());
//! ```
// Keep the public surface here; implementation modules follow the scan pipeline.
mod checker;
mod error;
mod lexer;
mod limits;
mod rules;
mod scores;
mod stack;

pub use checker::{Checker, analyze, check};
pub use error::{Reason, Rejection};
pub use limits::Limits;
pub use scores::{Report, Scores, Storage, StorageError};
