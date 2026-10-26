//! Optional score thresholds. No implementation ceilings are imposed.

/// `None` leaves a score unrestricted. Defaults collect/check without thresholds.
/// Estimated depths model syntax shape, not a guarantee about a particular parser.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_source_bytes: Option<usize>,
    pub max_tokens: Option<usize>,
    /// Resource budget: cumulative scanned bytes plus emitted tokens.
    pub max_work_units: Option<usize>,
    pub max_token_bytes: Option<usize>,
    /// Estimated parser recursion, including operators and indented suites.
    pub max_nesting_depth: Option<usize>,
    pub max_delimiter_depth: Option<usize>,
    pub max_indentation_depth: Option<usize>,
    pub max_annotation_depth: Option<usize>,
    pub check_string_annotations: bool,
    pub max_ast_depth: Option<usize>,
    pub max_call_arguments: Option<usize>,
    pub max_parameters: Option<usize>,
    pub max_display_items: Option<usize>,
    pub max_slice_items: Option<usize>,
    pub max_interpolation_fields: Option<usize>,
    pub max_boolean_operands: Option<usize>,
    pub max_with_items: Option<usize>,
    pub max_comprehension_filters: Option<usize>,
}

impl Limits {
    /// A starting policy for pre-parser screening; tune it for your parser.
    /// Unlike `Default`, this enables score thresholds and annotation scanning.
    pub fn screening() -> Self {
        Self {
            max_source_bytes: Some(1024 * 1024),
            max_tokens: Some(100_000),
            max_work_units: Some(1024 * 1024 + 100_000),
            max_nesting_depth: Some(100),
            max_indentation_depth: Some(80),
            max_annotation_depth: Some(8),
            check_string_annotations: true,
            max_ast_depth: Some(100),
            max_boolean_operands: Some(4_096),
            max_with_items: Some(1_024),
            max_comprehension_filters: Some(8_192),
            ..Self::default()
        }
    }

    #[inline]
    pub(crate) fn widths_enabled(&self) -> bool {
        self.max_call_arguments.is_some()
            || self.max_parameters.is_some()
            || self.max_display_items.is_some()
            || self.max_slice_items.is_some()
            || self.max_interpolation_fields.is_some()
            || self.max_boolean_operands.is_some()
            || self.max_with_items.is_some()
            || self.max_comprehension_filters.is_some()
    }

    #[inline]
    pub(crate) fn wide_expressions_enabled(&self) -> bool {
        self.max_boolean_operands.is_some()
            || self.max_with_items.is_some()
            || self.max_comprehension_filters.is_some()
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: None,
            max_tokens: None,
            max_work_units: None,
            max_token_bytes: None,
            max_nesting_depth: None,
            max_delimiter_depth: None,
            max_indentation_depth: None,
            max_annotation_depth: None,
            check_string_annotations: true,
            max_ast_depth: None,
            max_call_arguments: None,
            max_parameters: None,
            max_display_items: None,
            max_slice_items: None,
            max_interpolation_fields: None,
            max_boolean_operands: None,
            max_with_items: None,
            max_comprehension_filters: None,
        }
    }
}
