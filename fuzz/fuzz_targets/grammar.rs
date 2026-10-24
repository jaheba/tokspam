#![no_main]
#[path = "../../tools/generator.rs"]
mod generator;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 11 {
        return;
    }
    let family = generator::FAMILIES[data[0] as usize % generator::FAMILIES.len()];
    let depth = u16::from_le_bytes([data[1], data[2]]) as usize;
    let seed = u64::from_le_bytes(data[3..11].try_into().unwrap());
    let source = generator::generate(family, depth, seed);
    if let Err(error) = spam::check(&source, &spam::Limits::default()) {
        assert!(source.get(error.range).is_some());
    }
});
