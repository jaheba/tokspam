#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    const FIELDS: usize = 12;
    const CONFIG_BYTES: usize = FIELDS * 4;
    if data.len() < CONFIG_BYTES {
        return;
    }
    let value = |index: usize, maximum: usize| {
        let start = index * 4;
        let raw = u32::from_le_bytes(data[start..start + 4].try_into().unwrap()) as usize;
        if raw & 1 == 0 {
            (raw >> 1) % (maximum + 1)
        } else {
            maximum + 1
        }
    };
    let optional = |index: usize, maximum: usize| {
        (data[index * 4] & 2 == 0).then(|| value(index, maximum))
    };
    let limits = spam::Limits {
        max_source_bytes: value(0, spam::MAX_SOURCE_BYTES),
        max_tokens: value(1, spam::MAX_TOKENS),
        max_token_bytes: optional(2, spam::MAX_TOKEN_BYTES),
        max_nesting_depth: value(3, usize::from(spam::MAX_NESTING_DEPTH)) as u16,
        max_indentation_depth: value(4, usize::from(spam::MAX_INDENTATION_DEPTH)) as u16,
        max_annotation_depth: value(5, usize::from(spam::MAX_ANNOTATION_DEPTH)) as u8,
        check_string_annotations: data[0] & 4 == 0,
        max_ast_depth: optional(6, usize::from(spam::MAX_AST_DEPTH)).map(|value| value as u16),
        max_call_arguments: optional(7, spam::MAX_CONSTRUCT_WIDTH),
        max_parameters: optional(8, spam::MAX_CONSTRUCT_WIDTH),
        max_display_items: optional(9, spam::MAX_CONSTRUCT_WIDTH),
        max_slice_items: optional(10, spam::MAX_CONSTRUCT_WIDTH),
        max_interpolation_fields: optional(11, spam::MAX_CONSTRUCT_WIDTH),
    };
    if let Ok(source) = std::str::from_utf8(&data[CONFIG_BYTES..]) {
        if let Err(error) = spam::check(source, &limits) {
            assert!(source.get(error.range).is_some());
        }
    }
});
