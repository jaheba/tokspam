//! Plain strings and the text portions of f-strings and template strings.

use super::bytes::find_ascii;
use super::{Context, Kind, Lexer, Token};

impl Lexer<'_, '_> {
    pub(super) fn quoted(&mut self, start: usize, quote_start: usize) -> Token {
        let bytes = self.source.as_bytes();
        let quote = bytes[quote_start];
        let width = if bytes.get(quote_start..quote_start + 3) == Some(&[quote; 3]) {
            3
        } else {
            1
        };
        self.position = quote_start + width;
        let prefix = &bytes[start..quote_start];
        // F/t-strings hand control back to the main iterator so replacement
        // expressions count toward the same delimiter and depth budgets.
        if prefix
            .iter()
            .any(|b| matches!(b, b'f' | b'F' | b't' | b'T'))
        {
            let kind = if self.push(Context::Text {
                quote,
                width: width as u8,
            }) {
                Kind::FStringStart
            } else {
                Kind::NestingLimit
            };
            return self.token(kind, start);
        }

        // Plain strings are a single token. Retain only their interior range;
        // the annotation scanner decides whether those bytes need another scan.
        let inner = self.position;
        while self.position < bytes.len() {
            let text = &bytes[self.position..];
            let offset = if width == 1 {
                find_ascii(text, [quote, b'\\', b'\n', b'\r'])
            } else {
                find_ascii(text, [quote, b'\\'])
            };
            self.position += offset;
            if self.position == bytes.len() {
                break;
            }
            let byte = bytes[self.position];
            if byte == quote
                && (width == 1 || bytes.get(self.position..self.position + 3) == Some(&[quote; 3]))
            {
                let end = self.position;
                self.position += width;
                let mut token = self.token(Kind::String, start);
                if !prefix.iter().any(|b| matches!(b, b'b' | b'B')) {
                    token.contents = Some(inner..end);
                }
                return token;
            }
            if width == 1 && matches!(byte, b'\n' | b'\r') {
                // Resume code on the next line after an unterminated short string.
                break;
            }
            self.position += 1;
            if byte == b'\\' && self.position < bytes.len() {
                // Skip an escaped scalar without decoding it or leaving the
                // cursor in the middle of a multibyte UTF-8 character.
                self.position += self.source[self.position..]
                    .chars()
                    .next()
                    .unwrap()
                    .len_utf8();
            }
        }
        self.stopped = true;
        self.token(Kind::Limit, start)
    }

    // Keep text handling out of the ordinary token dispatch loop.
    #[inline(never)]
    // None means text was consumed without emitting a token, not EOF. The
    // main iterator loops again and alone decides whether the scan has ended.
    pub(super) fn text_token(&mut self, context: Context) -> Option<Token> {
        let bytes = self.source.as_bytes();
        let start = self.position;
        let byte = bytes[start];

        // Literal text has no structural effect until an ASCII delimiter.
        // Stopping only at ASCII or EOF also preserves UTF-8 boundaries.
        let text_run = match context {
            Context::Text { quote, .. } => find_ascii(&bytes[start..], [quote, b'{', b'}', b'\\']),
            Context::Format => find_ascii(&bytes[start..], *b"{}\\"),
            _ => 0,
        };
        if text_run != 0 {
            self.position += text_run;
            return None;
        }

        if let Context::Text { quote, width } = context {
            if byte == quote && (width == 1 || bytes.get(start..start + 3) == Some(&[quote; 3])) {
                self.position += usize::from(width);
                self.contexts.pop();
                return Some(self.token(Kind::FStringEnd, start));
            }
        }
        if byte == b'{' {
            if matches!(context, Context::Text { .. }) && bytes.get(start + 1) == Some(&b'{') {
                self.position += 2;
                return None;
            }
            self.position += 1;
            let kind = if self.push(Context::Field) {
                Kind::Lbrace
            } else {
                Kind::NestingLimit
            };
            return Some(self.token(kind, start));
        }
        if byte == b'}' && matches!(context, Context::Format) {
            self.position += 1;
            self.contexts.pop();
            return Some(self.token(Kind::Rbrace, start));
        }
        if byte == b'}' && bytes.get(start + 1) == Some(&b'}') {
            self.position += 2;
            return None;
        }
        // Keep braces visible after backslashes: escapes such as \N{...}
        // are not decoded, so this remains conservative structural screening.
        if byte == b'\\'
            && bytes
                .get(start + 1)
                .is_some_and(|b| matches!(b, b'\\' | b'\'' | b'"'))
        {
            self.position += 2;
        } else {
            self.position += self.source[start..].chars().next().unwrap().len_utf8();
        }
        None
    }
}
