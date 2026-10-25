//! Lexer regressions: byte-search equivalence, literal boundaries, and token ranges.

use super::bytes::find_ascii;
use super::*;

fn tokens(source: &str) -> Vec<Token> {
    let mut scratch = LexerScratch::default();
    let tokens: Vec<_> = Lexer::new(source, 100, 100, &mut scratch).collect();
    for token in &tokens {
        assert!(source.is_char_boundary(token.range.start));
        assert!(source.is_char_boundary(token.range.end));
    }
    tokens
}

#[test]
fn byte_search_matches_scalar_at_every_alignment_and_byte_value() {
    fn compare<const N: usize>(text: &[u8], needles: [u8; N]) {
        let scalar = text
            .iter()
            .position(|byte| needles.contains(byte))
            .unwrap_or(text.len());
        assert_eq!(find_ascii(text, needles), scalar, "{text:?}: {needles:?}");
    }

    for length in 0..80 {
        for byte in 0..=u8::MAX {
            let mut text = vec![byte; length];
            for position in 0..=length {
                if position < length {
                    text[position] = b'\\';
                }
                compare(&text, *b"\"\\");
                compare(&text, *b"{}\\");
                compare(&text, *b"'\\\n\r");
                if position < length {
                    text[position] = byte;
                }
            }
        }
    }

    let mut random = 42u64;
    for length in 0..512 {
        let text: Vec<_> = (0..length)
            .map(|_| {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                (random >> 32) as u8
            })
            .collect();
        compare(&text, *b"\"\\");
        compare(&text, *b"{}\\");
        compare(&text, *b"'\\\n\r");
    }
}

#[test]
fn literal_word_boundaries_preserve_unicode_and_escaped_delimiters() {
    for padding in 0..40 {
        for quote in ["'", "\"", "'''", "\"\"\""] {
            let body = format!("{}🦀π\\{quote}tail", "x".repeat(padding));
            let source = format!("x = {quote}{body}{quote}\n");
            let result = tokens(&source);
            let string = result
                .iter()
                .find(|token| token.kind == Kind::String)
                .unwrap();
            assert_eq!(string.range, 4..source.len() - 1);
            assert_eq!(
                string.contents,
                Some(4 + quote.len()..source.len() - 1 - quote.len())
            );
        }
        for ending in ["\n", "\r", "\r\n", ""] {
            let source = format!("x = '{}🦀{ending}", "x".repeat(padding));
            assert!(
                tokens(&source)
                    .iter()
                    .any(|token| token.kind == Kind::Limit)
            );
        }
    }
}

#[test]
fn text_runs_preserve_fields_escapes_and_quote_boundaries() {
    for prefix in ["f", "t"] {
        for quote in ["\"", "'''", "\"\"\""] {
            let source = format!("{prefix}{quote}🦀{{{{literal}}}}\\N{{value}}tail{quote}");
            let result = tokens(&source);
            assert_eq!(
                result.iter().map(|token| token.kind).collect::<Vec<_>>(),
                [
                    Kind::FStringStart,
                    Kind::Lbrace,
                    Kind::Name,
                    Kind::Rbrace,
                    Kind::FStringEnd
                ]
            );
            assert_eq!(&source[result[2].range.clone()], "value");
            assert_eq!(&source[result[4].range.clone()], quote);
        }
    }

    let source = "f\"🦀\\\"text{x:🦀>{width}}tail\"";
    assert_eq!(
        tokens(source)
            .iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>(),
        [
            Kind::FStringStart,
            Kind::Lbrace,
            Kind::Name,
            Kind::Colon,
            Kind::Lbrace,
            Kind::Name,
            Kind::Rbrace,
            Kind::Rbrace,
            Kind::FStringEnd
        ]
    );

    for source in ["f\"🦀text", "f'''🦀'text''", "f\"{x:🦀text"] {
        let result = tokens(source);
        let last = result.last().unwrap();
        assert_eq!(last.kind, Kind::Limit);
        assert_eq!(last.range, source.len()..source.len());
    }
}

#[test]
fn whitespace_runs_preserve_indentation_and_token_ranges() {
    let source = "if x:\n \t\x0c  y \t\x0c = 1\nz\n";
    let result = tokens(source);
    let indent = result
        .iter()
        .find(|token| token.kind == Kind::Indent)
        .unwrap();
    assert_eq!(indent.range, 6..source.find('y').unwrap());
    let dedent = result
        .iter()
        .find(|token| token.kind == Kind::Dedent)
        .unwrap();
    let end = source.find('z').unwrap();
    assert_eq!(dedent.range, end..end);
    let equal = result
        .iter()
        .find(|token| token.kind == Kind::Equal)
        .unwrap();
    assert_eq!(&source[equal.range.clone()], "=");
}
