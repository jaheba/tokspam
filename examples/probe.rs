//! Command-line adapter for isolated parser campaigns. Never executes input.
#[path = "../tools/generator.rs"]
mod generator;

use spam::{Limits, check};
use std::io::{self, Read};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("families") => println!("{}", generator::FAMILIES.join("\n")),
        Some("generate") => {
            let family = args.get(2).expect("family");
            assert!(generator::FAMILIES.contains(&family.as_str()));
            let depth = args.get(3).expect("depth").parse().expect("numeric depth");
            let seed = args.get(4).expect("seed").parse().expect("numeric seed");
            print!("{}", generator::generate(family, depth, seed));
        }
        Some("check") | Some("check-widths") => {
            let mut source = String::new();
            io::stdin()
                .take(1024 * 1024 + 1)
                .read_to_string(&mut source)
                .unwrap();
            let depth = args
                .get(2)
                .map(|s| s.parse().expect("numeric depth"))
                .unwrap_or(100);
            let limits = Limits {
                max_nesting_depth: depth,
                max_ast_depth: Some(depth),
                ..Limits::default()
            };
            #[cfg(feature = "experimental-width-limits")]
            let limits = if args[1] == "check-widths" {
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
            #[cfg(not(feature = "experimental-width-limits"))]
            assert!(
                args[1] != "check-widths",
                "rebuild with experimental-width-limits"
            );
            match check(&source, &limits) {
                Ok(()) => println!("accept"),
                Err(error) => println!(
                    "reject:{:?}:{}:{}",
                    error.reason, error.range.start, error.range.end
                ),
            }
        }
        _ => panic!(
            "expected families, generate FAMILY DEPTH SEED, check [DEPTH], or check-widths [DEPTH]"
        ),
    }
}
