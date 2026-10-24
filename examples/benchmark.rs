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

    let result = check(&source, &limits);
    let status = if result.is_ok() { "accept" } else { "reject" };
    let ns = if std::env::args().any(|arg| arg == "--guard") {
        let mut guard = byte_guard::ByteGuard::new(limits.clone());
        let mut checker = Checker::new(limits);
        let proved = guard.proves_acceptance(&source);
        assert!(!proved || result.is_ok(), "invalid byte-guard proof");
        eprintln!("byte-guard: {}", if proved { "proved" } else { "fallback" });

        bench_util::median_ns(|| {
            if !guard.proves_acceptance(black_box(&source)) {
                let _ = black_box(checker.check(black_box(&source)));
            }
        })
    } else if std::env::args().any(|arg| arg == "--reuse") {
        let mut checker = Checker::new(limits);

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
