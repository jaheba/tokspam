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
pub unsafe extern "C" fn demo_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }

    if let Ok(layout) = Layout::array::<u8>(len.max(1)) {
        unsafe { dealloc(ptr, layout) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn demo_check(ptr: *const u8, len: usize) -> u64 {
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

    match check(source, &Limits::default()) {
        Ok(()) => 0,
        Err(rejection) => encode(
            reason_code(rejection.reason),
            rejection.range.start,
            rejection.range.end,
        ),
    }
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
