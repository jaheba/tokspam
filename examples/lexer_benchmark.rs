//! Isolated lexer throughput and sampling target; pipe Python source on stdin.
#[path = "../tools/bench_util.rs"]
mod bench_util;
#[allow(dead_code)]
#[path = "../src/lexer.rs"]
mod lexer;

use std::{
    hint::black_box,
    io::{self, Read},
    time::{Duration, Instant},
};

fn main() {
    let mut source = String::new();
    io::stdin().read_to_string(&mut source).unwrap();
    let mut scratch = lexer::LexerScratch::new(1_024, 1_024);
    let mut scan = || {
        let mut count = 0;
        for token in lexer::Lexer::new(black_box(&source), 1_024, 1_024, &mut scratch) {
            black_box(token);
            count += 1;
        }
        black_box(count)
    };
    if std::env::args().any(|arg| arg == "--sample") {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            scan();
        }
    } else if std::env::args().any(|arg| arg == "--tokens") {
        for token in lexer::Lexer::new(&source, 1_024, 1_024, &mut scratch) {
            println!("{token:?}");
        }
    } else {
        let tokens = scan();
        let ns = bench_util::median_ns(|| {
            scan();
        });
        println!("{tokens},{ns:.1}");
    }
}
