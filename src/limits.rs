//! Configurable budgets, hard ceilings, and validation before scratch allocation.

pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TOKENS: usize = 1_000_000;
pub const MAX_WORK_UNITS: usize = MAX_SOURCE_BYTES + MAX_TOKENS;
pub const MAX_TOKEN_BYTES: usize = MAX_SOURCE_BYTES;
pub const MAX_NESTING_DEPTH: u16 = 1_024;
pub const MAX_INDENTATION_DEPTH: u16 = 1_024;
pub const MAX_ANNOTATION_DEPTH: u8 = 32;
pub const MAX_AST_DEPTH: u16 = 4_096;
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
    CallArguments,
    Parameters,
    DisplayItems,
    SliceItems,
    InterpolationFields,
    WorkUnits,
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
    /// Maximum cumulative scanned bytes plus emitted tokens.
    pub max_work_units: usize,
    pub max_token_bytes: Option<usize>,
    pub max_nesting_depth: u16,
    pub max_indentation_depth: u16,
    pub max_annotation_depth: u8,
    pub check_string_annotations: bool,
    /// Flat AST-shape estimates supplement the parser-recursion rules.
    pub max_ast_depth: Option<u16>,
    /// Optional flat-width budgets. `None` preserves the normal scanner path.
    pub max_call_arguments: Option<usize>,
    pub max_parameters: Option<usize>,
    pub max_display_items: Option<usize>,
    pub max_slice_items: Option<usize>,
    pub max_interpolation_fields: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            max_tokens: 100_000,
            max_work_units: 1024 * 1024 + 100_000,
            max_token_bytes: None,
            max_nesting_depth: 100,
            max_indentation_depth: 80,
            max_annotation_depth: 8,
            check_string_annotations: true,
            max_ast_depth: Some(100),
            max_call_arguments: None,
            max_parameters: None,
            max_display_items: None,
            max_slice_items: None,
            max_interpolation_fields: None,
        }
    }
}

impl Limits {
    /// Larger byte and token budgets for screening published package sources.
    /// Structural limits match `Default`; this is not a parser-safety guarantee.
    pub fn package_sources() -> Self {
        Self {
            max_source_bytes: 8 * 1024 * 1024,
            max_tokens: 500_000,
            max_work_units: 8 * 1024 * 1024 + 500_000,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_limit(
            self.max_source_bytes,
            MAX_SOURCE_BYTES,
            LimitKind::SourceBytes,
        )?;
        validate_limit(self.max_tokens, MAX_TOKENS, LimitKind::Tokens)?;
        validate_limit(self.max_work_units, MAX_WORK_UNITS, LimitKind::WorkUnits)?;
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

impl Limits {
    #[inline]
    pub(crate) fn widths_enabled(&self) -> bool {
        self.max_call_arguments.is_some()
            || self.max_parameters.is_some()
            || self.max_display_items.is_some()
            || self.max_slice_items.is_some()
            || self.max_interpolation_fields.is_some()
    }
}
