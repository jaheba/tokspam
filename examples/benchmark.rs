//! In-process Spam benchmark. Pipe one source file on stdin.
#[path = "../tools/bench_util.rs"]
mod bench_util;
#[path = "../tools/byte_guard.rs"]
mod byte_guard;

use spam::{Checker, Limits, check};
use std::{
    hint::black_box,
    io::{self, Read},
};

fn main() {
    let mut source = String::new();
    io::stdin().read_to_string(&mut source).unwrap();
    let limits = Limits::default();
    #[cfg(feature = "experimental-width-limits")]
    let limits = if std::env::args().any(|arg| arg == "--widths") {
        Limits {
            max_call_arguments: Some(4_096),
            max_parameters: Some(4_096),
            max_display_items: Some(8_192),
            max_slice_items: Some(4_096),
            max_interpolation_fields: Some(4_096),
            ..limits
        }
    } else {
        limits
    };
    if std::env::args().any(|arg| arg == "--guard-probe") {
        let mut guard = byte_guard::ByteGuard::new(limits.clone());
        let proved = guard.proves_acceptance(&source);
        assert!(
            !proved || check(&source, &limits).is_ok(),
            "invalid byte-guard proof"
        );
        println!("{}", if proved { "proved" } else { "fallback" });
        return;
    }

    #[cfg(feature = "experimental-regions")]
    if std::env::args().any(|arg| arg == "--regions-probe") {
        let mut checker = Checker::new(limits.clone()).expect("valid limits");
        let result = checker.check_regions(&source);
        assert_eq!(
            result,
            check(&source, &limits),
            "region fallback changed screening"
        );
        match result {
            Ok(()) => println!("accept"),
            Err(error) => println!(
                "reject:{:?}:{}:{}",
                error.reason, error.range.start, error.range.end
            ),
        }
        return;
    }

    let result = check(&source, &limits);
    let status = if result.is_ok() { "accept" } else { "reject" };
    let ns = if std::env::args().any(|arg| arg == "--regions") {
        #[cfg(feature = "experimental-regions")]
        {
            let mut checker = Checker::new(limits).expect("valid limits");
            assert_eq!(checker.check_regions(&source), result);
            bench_util::median_ns(|| {
                let _ = black_box(checker.check_regions(black_box(&source)));
            })
        }
        #[cfg(not(feature = "experimental-regions"))]
        panic!("rebuild with --features experimental-regions");
    } else if std::env::args().any(|arg| arg == "--guard") {
        let mut guard = byte_guard::ByteGuard::new(limits.clone());
        let mut checker = Checker::new(limits).expect("valid limits");
        let proved = guard.proves_acceptance(&source);
        assert!(!proved || result.is_ok(), "invalid byte-guard proof");
        eprintln!("byte-guard: {}", if proved { "proved" } else { "fallback" });

        bench_util::median_ns(|| {
            if !guard.proves_acceptance(black_box(&source)) {
                let _ = black_box(checker.check(black_box(&source)));
            }
        })
    } else if std::env::args().any(|arg| arg == "--reuse") {
        let mut checker = Checker::new(limits).expect("valid limits");

        bench_util::median_ns(|| {
            let _ = black_box(checker.check(black_box(&source)));
        })
    } else {
        bench_util::median_ns(|| {
            let _ = black_box(check(black_box(&source), black_box(&limits)));
        })
    };
    println!("spam,{status},{},{ns:.1}", source.len());
}
