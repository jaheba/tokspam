//! Measurements and explicit completion status.
use crate::Rejection;

/// Observed maxima and cumulative counts. In incomplete reports these are lower
/// bounds, except `source_bytes`, which always describes the entire input.
/// Annotation rescans contribute tokens/work and inherited structural depth.
/// Escaped annotations also charge their raw decoding pass to scanned bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scores {
    pub source_bytes: usize,
    pub tokens: usize,
    pub scanned_bytes: usize,
    pub work_units: usize,
    pub max_token_bytes: usize,
    pub delimiter_depth: usize,
    pub indentation_depth: usize,
    pub parser_depth: usize,
    pub ast_depth: usize,
    pub annotation_depth: usize,
    pub call_arguments: usize,
    pub parameters: usize,
    pub display_items: usize,
    pub slice_items: usize,
    pub interpolation_fields: usize,
    pub boolean_operands: usize,
    pub with_items: usize,
    pub comprehension_filters: usize,
}

/// `stopped` distinguishes a score violation from work/storage exhaustion,
/// arithmetic overflow, or a lexical structure the scanner cannot continue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub scores: Scores,
    pub stopped: Option<Rejection>,
}
impl Report {
    pub fn is_complete(&self) -> bool {
        self.stopped.is_none()
    }
}

/// Scratch policy, independent of input score thresholds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Storage {
    #[default]
    Growing,
    /// Reserve this many entries in each scratch stack and this many bytes for
    /// decoded annotation text. Scans never allocate;
    /// exhausting any stack returns `Storage` rather than rejecting a score.
    Fixed { capacity: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageError;
impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unable to reserve scratch storage")
    }
}
impl std::error::Error for StorageError {}
