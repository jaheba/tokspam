use std::alloc::{Layout, alloc, dealloc};

use tokspam::{Limits, Reason, check};

// Keep the reason separate from two full-width wasm32 byte offsets.
std::thread_local! { static RESULT_RANGE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
#[unsafe(no_mangle)]
pub extern "C" fn demo_result_range() -> u64 {
    RESULT_RANGE.with(|range| range.get())
}

#[unsafe(no_mangle)]
pub extern "C" fn demo_alloc(len: usize) -> *mut u8 {
    let Ok(layout) = Layout::array::<u8>(len.max(1)) else {
        return std::ptr::null_mut();
    };

    // JavaScript writes the bytes before asking Rust to read them.
    unsafe { alloc(layout) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must come from `demo_alloc(len)` and must not have been freed already.
pub unsafe extern "C" fn demo_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }

    if let Ok(layout) = Layout::array::<u8>(len.max(1)) {
        unsafe { dealloc(ptr, layout) };
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must point to `len` initialized bytes in WebAssembly memory.
pub unsafe extern "C" fn demo_check(ptr: *const u8, len: usize) -> u32 {
    unsafe { check_with_limits(ptr, len, &Limits::screening()) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `ptr` must point to `len` initialized bytes in WebAssembly memory.
pub unsafe extern "C" fn demo_check_configured(
    ptr: *const u8,
    len: usize,
    max_source_bytes: u32,
    max_tokens: u32,
    max_nesting_depth: u32,
    max_indentation_depth: u32,
    max_ast_depth: u32,
    max_annotation_depth: u32,
) -> u32 {
    let limits = Limits {
        max_source_bytes: Some(max_source_bytes as usize),
        max_tokens: Some(max_tokens as usize),
        // If the sum is not representable on wasm32, omit this optional budget.
        max_work_units: (max_source_bytes as usize).checked_add(max_tokens as usize),
        max_nesting_depth: Some(max_nesting_depth as usize),
        max_indentation_depth: Some(max_indentation_depth as usize),
        max_ast_depth: Some(max_ast_depth as usize),
        max_annotation_depth: Some(max_annotation_depth as usize),
        ..Limits::screening()
    };

    unsafe { check_with_limits(ptr, len, &limits) }
}

/// The six fields are source bytes, tokens, nesting, indentation, AST, and annotation depth.
#[unsafe(no_mangle)]
pub extern "C" fn demo_setting_default(field: u32) -> u32 {
    let limits = Limits::screening();

    match field {
        0 => limits.max_source_bytes.unwrap_or(0) as u32,
        1 => limits.max_tokens.unwrap_or(0) as u32,
        2 => limits.max_nesting_depth.unwrap_or(0) as u32,
        3 => limits.max_indentation_depth.unwrap_or(0) as u32,
        4 => limits.max_ast_depth.map_or(0, |value| value as u32),
        5 => limits.max_annotation_depth.unwrap_or(0) as u32,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn demo_setting_maximum(_field: u32) -> u32 {
    // JavaScript passes u32 settings; this is an ABI range, not a Tokspam ceiling.
    u32::MAX
}

unsafe fn check_with_limits(ptr: *const u8, len: usize, limits: &Limits) -> u32 {
    RESULT_RANGE.with(|range| range.set(0));
    if ptr.is_null() {
        return encode(17, 0, 0);
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(source) = std::str::from_utf8(bytes) else {
        return encode(17, 0, len);
    };

    match check(source, limits) {
        Ok(()) => 0,
        Err(rejection) => encode(
            reason_code(rejection.reason),
            rejection.range.start,
            rejection.range.end,
        ),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn demo_default_limit(reason: u32) -> u32 {
    let limits = Limits::screening();

    // A zero value means this rejection has no single configured numeric threshold.
    match reason {
        1 => limits.max_source_bytes.unwrap_or(0) as u32,
        2 => limits.max_tokens.unwrap_or(0) as u32,
        3 => limits.max_token_bytes.unwrap_or(0) as u32,
        4 => limits.max_nesting_depth.unwrap_or(0) as u32,
        5 => limits.max_indentation_depth.unwrap_or(0) as u32,
        6 => limits.max_ast_depth.map_or(0, |value| value as u32),
        10 => limits.max_call_arguments.unwrap_or(0) as u32,
        11 => limits.max_parameters.unwrap_or(0) as u32,
        12 => limits.max_display_items.unwrap_or(0) as u32,
        13 => limits.max_slice_items.unwrap_or(0) as u32,
        14 => limits.max_interpolation_fields.unwrap_or(0) as u32,
        20 => limits.max_boolean_operands.unwrap_or(0) as u32,
        21 => limits.max_with_items.unwrap_or(0) as u32,
        22 => limits.max_comprehension_filters.unwrap_or(0) as u32,
        _ => 0,
    }
}

fn encode(reason: u32, start: usize, end: usize) -> u32 {
    RESULT_RANGE.with(|range| range.set(((start as u64) << 32) | end as u64));
    reason
}

fn reason_code(reason: Reason) -> u32 {
    match reason {
        Reason::SourceSize => 1,
        Reason::TokenCount => 2,
        Reason::TokenSize => 3,
        Reason::Nesting => 4,
        Reason::Indentation => 5,
        Reason::AstDepth => 6,
        Reason::LexicalStructure => 7,
        Reason::AnnotationEscape => 19,
        Reason::QuotedAnnotation => 8,
        Reason::Storage => 9,
        Reason::Overflow => 16,
        Reason::DelimiterDepth => 18,
        Reason::CallArguments => 10,
        Reason::Parameters => 11,
        Reason::DisplayItems => 12,
        Reason::SliceItems => 13,
        Reason::InterpolationFields => 14,
        Reason::BooleanOperands => 20,
        Reason::WithItems => 21,
        Reason::ComprehensionFilters => 22,
        Reason::Inconclusive => 15,
    }
}

#[cfg(test)]
mod tests {
    use super::demo_check_configured;

    #[test]
    fn configured_depth_changes_the_demo_result() {
        let source = format!("value = {}0{}\n", "[".repeat(120), "]".repeat(120));
        let reason = |nesting, ast| unsafe {
            demo_check_configured(
                source.as_ptr(),
                source.len(),
                1_048_576,
                100_000,
                nesting,
                80,
                ast,
                8,
            )
        };

        assert_eq!(reason(100, 100), 4);
        assert_eq!(reason(150, 150), 0);
        assert_eq!(reason(2_000, 100), 6);
    }
}

#[cfg(test)]
mod range_tests {
    #[test]
    fn full_wasm32_ranges_do_not_truncate() {
        let end = u32::MAX as usize;
        assert_eq!(super::encode(7, end - 1, end), 7);
        let range = super::demo_result_range();
        assert_eq!(range >> 32, (end - 1) as u64);
        assert_eq!(range & u64::from(u32::MAX), end as u64);
    }
}
