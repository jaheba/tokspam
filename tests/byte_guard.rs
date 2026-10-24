#[path = "../tools/byte_guard.rs"]
mod byte_guard;
#[path = "../tools/generator.rs"]
#[allow(dead_code)]
mod generator;

use byte_guard::ByteGuard;
use spam::{Checker, Limits};

#[test]
fn byte_guard_only_proves_inputs_the_checker_accepts() {
    let pieces = [
        "x", "1", " ", "\n", "\r\n", "\t", "\x0c", "(", ")", "[", "]", "{", "}", "'", "\"", "'''",
        "\"\"\"", "r", "b", "f", "t", "rb", "rf", "br", "fr", "λ", "🦀", "#", "\\", "\\\n",
        "\\\r\n", ":", ",", ";", "->", "+", "-", "*", "**", "/", "=", "elif", "else", "lambda",
        "async", "await", "yield", "not", "and", "if", ".", "!=",
    ];
    let mut random = 42u64;
    let mut proved = 0;

    for depth in [0, 1, 4, 30, 100] {
        for ast in [None, Some(0), Some(5), Some(100)] {
            for tokens in [0, 5, 100_000] {
                let limits = Limits {
                    max_nesting_depth: depth,
                    max_ast_depth: ast,
                    max_tokens: tokens,
                    ..Limits::default()
                };
                let mut guard = ByteGuard::new(limits.clone());
                let mut checker = Checker::new(limits);

                for _ in 0..1000 {
                    let mut source = String::new();
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let length = (random >> 32) as usize % 100;
                    for _ in 0..length {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        source.push_str(pieces[(random >> 32) as usize % pieces.len()]);
                    }
                    if guard.proves_acceptance(&source) {
                        proved += 1;
                        assert_eq!(checker.check(&source), Ok(()), "{source:?}");
                    }
                }
            }
        }
    }
    assert!(proved > 1000, "exercise positive proofs, not only fallback");
}

#[test]
fn generated_shapes_and_layout_boundaries_fall_back_safely() {
    for depth in [0, 1, 4, 30, 100] {
        let limits = Limits {
            max_nesting_depth: depth,
            max_ast_depth: Some(depth),
            max_indentation_depth: depth,
            ..Limits::default()
        };
        let mut guard = ByteGuard::new(limits.clone());
        let mut checker = Checker::new(limits);

        for family in generator::FAMILIES {
            for size in [1, 10, 80, 101, 300] {
                let source = generator::generate(family, size, 42);
                if guard.proves_acceptance(&source) {
                    assert_eq!(
                        checker.check(&source),
                        Ok(()),
                        "{family}({size}), limit {depth}"
                    );
                }
            }
        }
        for source in [
            " \\\n",
            " \\\r\n",
            "x:\n \\\n",
            "x:\n pass\n  \\\n",
            "x:\n\tpass\n # blank\nx = 1\n",
        ] {
            if guard.proves_acceptance(source) {
                assert_eq!(checker.check(source), Ok(()), "{source:?}");
            }
        }
    }
}
