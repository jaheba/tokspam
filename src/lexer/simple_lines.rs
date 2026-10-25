//! Conservative acceptance shortcuts for a prefix of simple logical lines.

use super::Lexer;

impl Lexer<'_, '_> {
    // Only an unindented prefix of standalone ASCII lines qualifies. Strings,
    // delimiters, annotations and continuations use the detailed lexer instead.
    // This routine commits its position only after every bound has passed.
    pub(crate) fn skip_simple_line(
        &mut self,
        max_bytes: usize,
        max_tokens: usize,
        max_token_bytes: Option<usize>,
    ) -> Option<usize> {
        let bytes = self.source.as_bytes();
        let mut position = self.position;
        let mut column = 0;
        while let Some(byte) = bytes.get(position) {
            match byte {
                b' ' => column += 1,
                b'\t' => column = (column / 8 + 1) * 8,
                0x0c => column = 0,
                _ => break,
            }
            position += 1;
        }
        if column != 0 {
            return None;
        }

        let start = position;
        let mut tokens = 0;
        loop {
            if position - start > max_bytes {
                return None;
            }
            let token_start = position;
            let byte = *bytes.get(position)?;
            position += 1;
            let newline = match byte {
                b' ' | b'\t' | 0x0c => continue,
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    while bytes
                        .get(position)
                        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                    {
                        position += 1;
                    }
                    // These keywords either retain cross-line state or add
                    // multiple frames, invalidating the simple byte bound.
                    if matches!(&bytes[token_start..position], b"lambda" | b"elif" | b"else") {
                        return None;
                    }
                    false
                }
                b'0'..=b'9' => {
                    while bytes
                        .get(position)
                        .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
                    {
                        position += 1;
                    }
                    false
                }
                b'*' | b'/' | b'<' | b'>' | b'=' | b'!' => {
                    if matches!(
                        (byte, bytes.get(position)),
                        (b'*', Some(b'*'))
                            | (b'/', Some(b'/'))
                            | (b'<', Some(b'<' | b'='))
                            | (b'>', Some(b'>' | b'='))
                            | (b'=', Some(b'='))
                            | (b'!', Some(b'='))
                    ) {
                        position += 1;
                    }
                    false
                }
                b'+' | b'-' | b'~' | b'%' | b'@' | b'|' | b'&' | b'^' | b'.' | b',' => false,
                b'\n' | b'\r' => {
                    if byte == b'\r' && bytes.get(position) == Some(&b'\n') {
                        position += 1;
                    }
                    true
                }
                _ => return None,
            };
            tokens += 1;
            if tokens > max_tokens
                || max_token_bytes.is_some_and(|limit| position - token_start > limit)
            {
                return None;
            }
            if newline {
                if tokens == 1 {
                    return None;
                }
                self.position = position;
                return Some(tokens);
            }
        }
    }
}
