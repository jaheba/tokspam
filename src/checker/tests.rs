//! End-to-end regressions for the scan coordinator.

use super::*;
use crate::lexer::LexerScratch;

fn check(source: &str) -> Result<(), Rejection> {
    super::check(
        source,
        &Limits {
            max_source_bytes: Some(1_000_000),
            max_nesting_depth: Some(30),
            max_ast_depth: None,
            ..Limits::screening()
        },
    )
}

#[test]
fn fstrings_and_templates() {
    for source in [
        "f'{x}'",
        "rf'{x!r:>10}'",
        "t'{x}'",
        "f'{{literal}} {a[0]:{width}.{precision}f}'",
        "f\"{f'{x}'}\"",
        "f'{\"same quote\"}'",
        "f'{ {\"key\": value} }'",
        "f'{x:{{}}}'",
    ] {
        assert!(check(source).is_ok(), "{source}");
    }
    assert!(check(&format!("f'{}{{x}}{}'", "{x:".repeat(50), "}".repeat(50))).is_err());
    assert!(check(&format!("f'{{ {}1 }}'", "-".repeat(50))).is_err());
    assert!(check(&format!("f'{}'", "literal {{text}} ".repeat(1_000))).is_ok());
}

#[test]
fn indentation_and_continuations() {
    let deep: String = (0..40)
        .map(|i| format!("{}if x:\n", " ".repeat(i)))
        .collect();
    assert!(check(&deep).is_err());
    assert!(check(&"if x:\n\tpass\nelse:\n\tpass\n".repeat(100)).is_ok());
    assert!(check(&format!("x = (\n{}1\n)\n", " ".repeat(1000))).is_ok());
    assert!(check(&format!("x = {}1", "-\\\n".repeat(50))).is_err());
    assert!(check("if x:\r\n\tpass\r\n# comment\r\nx = 1\r\n").is_ok());
}

#[test]
fn quoted_annotations_and_bytes() {
    for quote in ["'", "\"", "'''", "\"\"\""] {
        assert!(
            check(&format!(
                "x: {quote}{}int{}{quote}",
                "[".repeat(50),
                "]".repeat(50)
            ))
            .is_err()
        );
    }
    assert!(check(&format!("x = b'{}'", "(".repeat(100))).is_ok());
    assert!(check("x: \"list['int']\"").is_ok());
    assert!(check("x: \"\"\"'''\"'int'\"'''\"\"\"").is_ok());
}

#[test]
fn malformed_and_unicode() {
    for source in [
        "π = '🦀'",
        "'unterminated\\",
        "\"\"\"",
        "\\\n",
        "\0🦀",
        "f'{",
        "([)]",
    ] {
        let mut scratch = LexerScratch::default();
        for token in Lexer::new(source, 30, 30, &mut scratch) {
            assert!(source.is_char_boundary(token.range.start));
            assert!(source.is_char_boundary(token.range.end));
        }
    }
    // An unterminated short string cannot hide code on following lines.
    assert!(check(&format!("'broken\n{}1", "-".repeat(50))).is_err());
}

#[test]
fn seeded_malformed_sources_terminate() {
    let alphabet = [
        'a', 'π', '🦀', '0', 'f', 'r', '\'', '"', '{', '}', '(', ')', '[', ']', ':', '\\', '\n',
        '\r', '\t', '#', '-', '*',
    ];
    let mut state = 42u64;
    for _ in 0..2_000 {
        let mut source = String::new();
        for _ in 0..100 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push(alphabet[(state >> 32) as usize % alphabet.len()]);
        }
        let mut scratch = LexerScratch::default();
        let tokens: Vec<_> = Lexer::new(&source, 30, 30, &mut scratch)
            .take(source.len() * 2 + 32)
            .collect();
        assert!(tokens.len() < source.len() * 2 + 32);
        for token in tokens {
            assert!(source.is_char_boundary(token.range.start));
            assert!(source.is_char_boundary(token.range.end));
        }
        let _ = check(&source);
    }
}

#[test]
fn ordinary_flat_code() {
    assert!(check(&"x = [-1, -2]\n".repeat(100)).is_ok());
    assert!(check(&format!("x = [{}]", "-1, ".repeat(3000))).is_ok());
    assert!(check(&format!("x = {{{}}}", "'k': lambda a, b: 0, ".repeat(500))).is_ok());
}
