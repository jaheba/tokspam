#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        if let Err(error) = spam::check(source, &spam::Limits::default()) {
            assert!(source.get(error.range).is_some());
        }
    }
});
