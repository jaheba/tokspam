//! Decode only annotation literals that contain escapes. Ordinary source stays borrowed.
//!
//! Inputs may already live in the arena. Read by index before appending each byte:
//! a growing arena can relocate, so holding a slice into it across a push is unsafe.
use crate::Reason;
use std::ops::Deref;
use std::ops::Range;

/// Append-only UTF-8 scratch. Parent slices remain valid across child appends;
/// every push preserves UTF-8, so slicing on resume requires no validation pass.
pub(super) struct DecodedText {
    text: String,
    capacity_limit: usize,
    failed: bool,
}

impl Default for DecodedText {
    fn default() -> Self {
        Self::with_storage(0, false)
    }
}

impl DecodedText {
    pub(super) fn with_storage(capacity: usize, fixed: bool) -> Self {
        let mut text = String::new();
        let failed = fixed && text.try_reserve_exact(capacity).is_err();
        Self {
            text,
            capacity_limit: if fixed { capacity } else { usize::MAX },
            failed,
        }
    }

    pub(super) fn failed(&self) -> bool {
        self.failed
    }

    pub(super) fn reset(&mut self) {
        self.text.clear();
        self.failed = false;
    }

    pub(super) fn truncate(&mut self, length: usize) {
        self.text.truncate(length);
    }

    fn push(&mut self, character: char) -> Result<(), Reason> {
        let needed = self
            .text
            .len()
            .checked_add(character.len_utf8())
            .ok_or(Reason::Overflow)?;
        if needed > self.capacity_limit {
            return Err(Reason::Storage);
        }
        if needed > self.text.capacity() {
            self.grow(needed)?;
        }
        self.text.push(character);
        Ok(())
    }

    #[cold]
    fn grow(&mut self, needed: usize) -> Result<(), Reason> {
        let target = self
            .text
            .capacity()
            .saturating_mul(2)
            .max(4)
            .max(needed)
            .min(self.capacity_limit);
        self.text
            .try_reserve_exact(target - self.text.len())
            .map_err(|_| Reason::Storage)
    }
}

impl Deref for DecodedText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}

struct Input<'a> {
    original: &'a str,
    range: Range<usize>,
    decoded: bool,
}

impl Input<'_> {
    fn character(&self, arena: &DecodedText, position: usize) -> char {
        let text = if self.decoded {
            &arena[self.range.clone()]
        } else {
            &self.original[self.range.clone()]
        };
        text[position - self.range.start..]
            .chars()
            .next()
            .expect("input character boundary")
    }

    fn byte(&self, arena: &DecodedText, position: usize) -> Option<u8> {
        if position >= self.range.end {
            None
        } else if self.decoded {
            arena.as_bytes().get(position).copied()
        } else {
            self.original.as_bytes().get(position).copied()
        }
    }
}

fn emit(arena: &mut DecodedText, byte: u8) -> Result<(), Reason> {
    arena.push(char::from(byte))
}

fn scalar(arena: &mut DecodedText, value: u32) -> Result<(), Reason> {
    // Python strings can contain isolated surrogates; Rust str cannot. Report
    // that unsupported representation explicitly instead of replacing a character.
    let character = char::from_u32(value).ok_or(if value <= 0x10ffff {
        Reason::AnnotationEscape
    } else {
        Reason::LexicalStructure
    })?;
    arena.push(character)
}

pub(super) fn decode(
    original: &str,
    range: Range<usize>,
    decoded: bool,
    arena: &mut DecodedText,
) -> Result<Range<usize>, Reason> {
    let input = Input {
        original,
        range,
        decoded,
    };
    let start = arena.len();
    let mut position = input.range.start;
    while let Some(byte) = input.byte(arena, position) {
        position += 1;
        if byte != b'\\' {
            if byte.is_ascii() {
                emit(arena, byte)?;
            } else {
                let character = input.character(arena, position - 1);
                position += character.len_utf8() - 1;
                arena.push(character)?;
            }
            continue;
        }
        let escape = input
            .byte(arena, position)
            .ok_or(Reason::LexicalStructure)?;
        position += 1;
        match escape {
            b'\\' | b'\'' | b'"' => emit(arena, escape)?,
            b'a' => emit(arena, 7)?,
            b'b' => emit(arena, 8)?,
            b'f' => emit(arena, 12)?,
            b'n' => emit(arena, b'\n')?,
            b'r' => emit(arena, b'\r')?,
            b't' => emit(arena, b'\t')?,
            b'v' => emit(arena, 11)?,
            // Physical escaped newlines disappear from a non-raw string value.
            b'\n' => {}
            b'\r' => {
                if input.byte(arena, position) == Some(b'\n') {
                    position += 1;
                }
            }
            b'0'..=b'7' => {
                let mut value = u32::from(escape - b'0');
                for _ in 0..2 {
                    match input.byte(arena, position) {
                        Some(next @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(next - b'0');
                            position += 1;
                        }
                        _ => break,
                    }
                }
                scalar(arena, value)?;
            }
            b'x' | b'u' | b'U' => {
                let digits = match escape {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let mut value = 0u32;
                for _ in 0..digits {
                    let digit = input
                        .byte(arena, position)
                        .and_then(|byte| char::from(byte).to_digit(16))
                        .ok_or(Reason::LexicalStructure)?;
                    value = value * 16 + digit;
                    position += 1;
                }
                scalar(arena, value)?;
            }
            // Resolving Unicode names requires a Unicode-name database. Do not
            // guess or ignore potentially structural characters such as brackets.
            b'N' => return Err(Reason::AnnotationEscape),
            _ => {
                // Python preserves unrecognized escapes. Keep the backslash,
                // including before a non-ASCII scalar whose later bytes follow.
                emit(arena, b'\\')?;
                let character = input.character(arena, position - 1);
                position += character.len_utf8() - 1;
                arena.push(character)?;
            }
        }
    }
    Ok(start..arena.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_escape_values_preserve_utf8_and_unknown_escapes() {
        let cases = [
            (r"List[\'int\']", "List['int']"),
            (r"\x27\u03c0\U0001f600\x27", "'π😀'"),
            (r"\141\142\143\777", "abcǿ"),
            (r"\\", "\\"),
            (r"\q", r"\q"),
            ("π\\\r\n🦀", "π🦀"),
            (r"\a\b\f\n\r\t\v", "\u{7}\u{8}\u{c}\n\r\t\u{b}"),
        ];
        for (source, expected) in cases {
            let mut arena = DecodedText::default();
            let range = decode(source, 0..source.len(), false, &mut arena).unwrap();
            assert_eq!(&arena[range], expected, "{source:?}");
        }
    }

    #[test]
    fn appending_to_the_input_arena_survives_reallocation() {
        let original = r"\x27\u03c0\x27";
        let mut arena = DecodedText::default();
        for character in original.chars() {
            arena.push(character).unwrap();
        }
        let result = decode("", 0..original.len(), true, &mut arena).unwrap();
        assert_eq!(&arena[..original.len()], original);
        assert_eq!(&arena[result], "'π'");
    }

    #[test]
    fn malformed_and_unsupported_escapes_are_explicit_stops() {
        for source in [r"\x0", r"\xzz", r"\u012", r"\U00110000", "\\"] {
            assert_eq!(
                decode(source, 0..source.len(), false, &mut DecodedText::default()),
                Err(Reason::LexicalStructure)
            );
        }
        for source in [r"\N{LEFT SQUARE BRACKET}", r"\ud800"] {
            assert_eq!(
                decode(source, 0..source.len(), false, &mut DecodedText::default()),
                Err(Reason::AnnotationEscape)
            );
        }
    }
}
