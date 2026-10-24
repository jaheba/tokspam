#![cfg(feature = "experimental-regions")]

#[path = "../tools/generator.rs"]
#[allow(dead_code)]
mod generator;

use spam::{Checker, Limits};

#[test]
fn region_fallback_matches_normal_screening() {
    let pieces = [
        "x = 1\n",
        "def f(x: 'int'):\n    return x + 1\n",
        "elif x:\n pass\n",
        "else:\n pass\n",
        "'docstring'\n",
        "f'{x:{w}}'\n",
        "x = (\n1 + 2)\n",
        "x = {'a': b'b'}\n",
        "lambda x\n",
        "lambda x: x, y\n",
        "x: \"list['int']\"\n",
        "x = 1; y = 2\n",
        "\t\x0cx = 1\r\n",
        "x = 1\\\n+ 2\n",
        "[)]\n",
        "'broken\n",
        "'''\nlong\nstring'''\n",
        "π = '🦀'\n",
        "# comment\n",
        "\n",
        "  # indented comment\n",
        "    \\\n",
        "y = 2",
    ];
    let mut random = 42u64;
    for depth in [0, 1, 4, 30, 100] {
        for ast in [None, Some(0), Some(5), Some(100)] {
            for tokens in [0, 5, 100_000] {
                for token_bytes in [None, Some(0), Some(4), Some(100)] {
                    let limits = Limits {
                        max_nesting_depth: depth,
                        max_indentation_depth: depth,
                        max_ast_depth: ast,
                        max_tokens: tokens,
                        max_token_bytes: token_bytes,
                        ..Limits::default()
                    };
                    let mut checker = Checker::new(limits).expect("valid limits");
                    for _ in 0..200 {
                        let mut source = String::new();
                        for _ in 0..10 {
                            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                            source.push_str(pieces[(random >> 32) as usize % pieces.len()]);
                        }
                        assert_eq!(
                            checker.check_regions(&source),
                            checker.check(&source),
                            "{source:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn generated_regions_match_normal_screening() {
    for depth in [0, 1, 4, 30, 100] {
        let limits = Limits {
            max_nesting_depth: depth,
            max_ast_depth: Some(depth),
            ..Limits::default()
        };
        let mut checker = Checker::new(limits).expect("valid limits");
        for family in generator::FAMILIES {
            for size in [1, 10, 80, 101, 300] {
                let source = format!("x = 1\n{}\ny = 2\n", generator::generate(family, size, 42));
                assert_eq!(
                    checker.check_regions(&source),
                    checker.check(&source),
                    "{family}({size}), limit {depth}"
                );
            }
        }
    }
}

#[test]
fn malformed_bytes_preserve_local_fallback_state() {
    let alphabet = [
        'a', 'π', '🦀', '0', 'f', 'r', '\'', '"', '{', '}', '(', ')', '[', ']', ':', '\\', '\n',
        '\r', '\t', '#', '-', '*', ';', ' ',
    ];
    let mut random = 123u64;
    for depth in [0, 1, 4, 30] {
        let mut checker = Checker::new(Limits {
            max_nesting_depth: depth,
            max_ast_depth: Some(depth),
            ..Limits::default()
        })
        .expect("valid limits");
        for _ in 0..5000 {
            let mut source = String::new();
            for _ in 0..100 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                source.push(alphabet[(random >> 32) as usize % alphabet.len()]);
            }
            assert_eq!(
                checker.check_regions(&source),
                checker.check(&source),
                "{source:?}"
            );
        }
    }
}

#[test]
fn annotation_fallback_keeps_recursive_budgets_and_outer_ranges() {
    let sources = [
        format!(
            "x: '{}int{}'\ny = 2\n",
            "list[".repeat(120),
            "]".repeat(120)
        ),
        format!("x: \"list['{}']\"\ny = 2\n", "word ".repeat(30)),
        "def f(x: \"list['int']\") -> 'int':\n    return x[0]\n".to_owned(),
        "x = f'{value}'\ny: 'int'\nz = 1\n".to_owned(),
    ];
    for limits in [
        Limits::default(),
        Limits {
            check_string_annotations: false,
            ..Limits::default()
        },
        Limits {
            max_annotation_depth: 0,
            ..Limits::default()
        },
        Limits {
            max_annotation_depth: 1,
            ..Limits::default()
        },
    ] {
        let mut checker = Checker::new(limits).expect("valid limits");
        for source in &sources {
            assert_eq!(
                checker.check_regions(source),
                checker.check(source),
                "{source:?}"
            );
        }
    }
}
