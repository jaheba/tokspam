use std::alloc::{Layout, alloc, dealloc};

use tokspam::{Limits, Reason, check};

const RANGE_BITS: u64 = 25;
const RANGE_MASK: u64 = (1 << RANGE_BITS) - 1;

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
pub unsafe extern "C" fn demo_check(ptr: *const u8, len: usize) -> u64 {
    unsafe { check_with_limits(ptr, len, &Limits::default()) }
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
) -> u64 {
    if max_source_bytes as usize > tokspam::MAX_SOURCE_BYTES
        || max_tokens as usize > tokspam::MAX_TOKENS
        || max_nesting_depth > u32::from(tokspam::MAX_NESTING_DEPTH)
        || max_indentation_depth > u32::from(tokspam::MAX_INDENTATION_DEPTH)
        || max_ast_depth > u32::from(tokspam::MAX_AST_DEPTH)
        || max_annotation_depth > u32::from(tokspam::MAX_ANNOTATION_DEPTH)
    {
        return encode(9, 0, 0);
    }

    let mut limits = Limits::default();
    limits.max_source_bytes = max_source_bytes as usize;
    limits.max_tokens = max_tokens as usize;
    // Keep the shared work budget aligned with the two editable volume budgets.
    limits.max_work_units = limits.max_source_bytes + limits.max_tokens;
    limits.max_nesting_depth = max_nesting_depth as u16;
    limits.max_indentation_depth = max_indentation_depth as u16;
    limits.max_ast_depth = Some(max_ast_depth as u16);
    limits.max_annotation_depth = max_annotation_depth as u8;

    unsafe { check_with_limits(ptr, len, &limits) }
}

/// The six fields are source bytes, tokens, nesting, indentation, AST, and annotation depth.
#[unsafe(no_mangle)]
pub extern "C" fn demo_setting_default(field: u32) -> u32 {
    let limits = Limits::default();

    match field {
        0 => limits.max_source_bytes as u32,
        1 => limits.max_tokens as u32,
        2 => u32::from(limits.max_nesting_depth),
        3 => u32::from(limits.max_indentation_depth),
        4 => limits.max_ast_depth.map_or(0, u32::from),
        5 => u32::from(limits.max_annotation_depth),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn demo_setting_maximum(field: u32) -> u32 {
    match field {
        0 => tokspam::MAX_SOURCE_BYTES as u32,
        1 => tokspam::MAX_TOKENS as u32,
        2 => u32::from(tokspam::MAX_NESTING_DEPTH),
        3 => u32::from(tokspam::MAX_INDENTATION_DEPTH),
        4 => u32::from(tokspam::MAX_AST_DEPTH),
        5 => u32::from(tokspam::MAX_ANNOTATION_DEPTH),
        _ => 0,
    }
}

unsafe fn check_with_limits(ptr: *const u8, len: usize, limits: &Limits) -> u64 {
    if len > tokspam::MAX_SOURCE_BYTES {
        return encode(1, 0, 0);
    }

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
    let limits = Limits::default();

    // A zero value means this rejection has no single configured numeric threshold.
    match reason {
        1 => limits.max_source_bytes as u32,
        2 => limits.max_tokens as u32,
        3 => limits.max_token_bytes.unwrap_or(0) as u32,
        4 => u32::from(limits.max_nesting_depth),
        5 => u32::from(limits.max_indentation_depth),
        6 => limits.max_ast_depth.map_or(0, u32::from),
        10 => limits.max_call_arguments.unwrap_or(0) as u32,
        11 => limits.max_parameters.unwrap_or(0) as u32,
        12 => limits.max_display_items.unwrap_or(0) as u32,
        13 => limits.max_slice_items.unwrap_or(0) as u32,
        14 => limits.max_interpolation_fields.unwrap_or(0) as u32,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn demo_source_ceiling() -> u32 {
    tokspam::MAX_SOURCE_BYTES as u32
}

fn encode(reason: u64, start: usize, end: usize) -> u64 {
    // Each range endpoint fits in 25 bits under the hard 16 MiB source ceiling.
    (reason << (RANGE_BITS * 2))
        | (((start as u64) & RANGE_MASK) << RANGE_BITS)
        | ((end as u64) & RANGE_MASK)
}

fn reason_code(reason: Reason) -> u64 {
    match reason {
        Reason::SourceSize => 1,
        Reason::TokenCount => 2,
        Reason::TokenSize => 3,
        Reason::Nesting => 4,
        Reason::Indentation => 5,
        Reason::AstDepth => 6,
        Reason::LexicalStructure => 7,
        Reason::QuotedAnnotation => 8,
        Reason::Configuration => 9,
        Reason::CallArguments => 10,
        Reason::Parameters => 11,
        Reason::DisplayItems => 12,
        Reason::SliceItems => 13,
        Reason::InterpolationFields => 14,
        Reason::Inconclusive => 15,
    }
}

#[cfg(test)]
mod tests {
    use super::demo_check_configured;

    #[test]
    fn configured_depth_changes_the_demo_result() {
        let source = format!("value = {}0{}\n", "[".repeat(120), "]".repeat(120));
        let reason = |nesting, ast| {
            let packed = unsafe {
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
            packed >> 50
        };

        assert_eq!(reason(100, 100), 4);
        assert_eq!(reason(150, 150), 0);
        assert_eq!(reason(2_000, 100), 9);
    }
}
