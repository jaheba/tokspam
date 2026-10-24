//! Parse in a disposable process; the campaign owns deadlines and memory limits.
use std::io::{self, Read};

#[path = "../../bench_util.rs"]
mod bench_util;

fn main() {
    let mut source = String::new();
    io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_string(&mut source)
        .unwrap();
    if source.len() > 1024 * 1024 {
        println!("source-limit");
        return;
    }
    if std::env::args().any(|arg| arg == "--bench") {
        let ns = bench_util::median_ns(|| {
            let _ = std::hint::black_box(ruff_python_parser::parse_module(std::hint::black_box(
                &source,
            )));
        });
        println!("ruff,parse,{},{ns:.1}", source.len());
        return;
    }
    // Retain the normal AST destructor: deep recursive drops are failures too.
    match ruff_python_parser::parse_module(&source) {
        Ok(parsed) => {
            drop(parsed);
            println!("ok");
        }
        Err(_) => println!("syntax-error"),
    }
}
