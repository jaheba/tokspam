//! Main token dispatch: code, layout, and transitions into literal text.

use super::{BYTE_CLASSES, Context, Kind, Lexer, NAME_BYTE, NUMBER_BYTE, Token};

impl Iterator for Lexer<'_, '_> {
    type Item = Token;

    #[inline(always)]
    fn next(&mut self) -> Option<Token> {
        if self.stopped {
            return None;
        }
        let bytes = self.source.as_bytes();
        loop {
            // Layout belongs to logical root lines. Bracketed expressions
            // continue across physical lines without adding block indentation.
            if self.line_start && matches!(self.contexts.last(), Some(Context::Root)) {
                if let Some(token) = self.indentation() {
                    return Some(token);
                }
            }
            if self.position >= bytes.len() {
                if self.contexts.len() > 1 {
                    self.stopped = true;
                    return Some(self.token(Kind::Limit, self.position));
                }
                return None;
            }
            let start = self.position;
            let byte = bytes[start];
            let context = *self.contexts.last().unwrap();

            if matches!(context, Context::Text { .. } | Context::Format) {
                if let Some(token) = self.text_token(context) {
                    return Some(token);
                }
                continue;
            }

            self.position += 1;
            let kind = match byte {
                b' ' | b'\t' | 0x0c => {
                    while bytes
                        .get(self.position)
                        .is_some_and(|b| matches!(b, b' ' | b'\t' | 0x0c))
                    {
                        self.position += 1;
                    }
                    continue;
                }
                b'#' => {
                    while self.position < bytes.len()
                        && !matches!(bytes[self.position], b'\n' | b'\r')
                    {
                        self.position += 1;
                    }
                    continue;
                }
                // An escaped newline continues the same statement: do not
                // set line_start or charge indentation on the following line.
                b'\\' if matches!(bytes.get(self.position), Some(b'\n' | b'\r')) => {
                    if bytes[self.position] == b'\r' {
                        self.position += 1;
                    }
                    if bytes.get(self.position) == Some(&b'\n') {
                        self.position += 1;
                    }
                    continue;
                }
                b'\n' | b'\r' => {
                    if byte == b'\r' && bytes.get(self.position) == Some(&b'\n') {
                        self.position += 1;
                    }
                    if matches!(context, Context::Root) {
                        self.line_start = true;
                        Kind::Newline
                    } else {
                        continue;
                    }
                }
                b'\'' | b'"' => return Some(self.quoted(start, start)),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' | 0x80..=0xff => {
                    // Group non-ASCII bytes without validating identifier
                    // spelling. Consuming the full run preserves UTF-8 ranges.
                    while self.position < bytes.len()
                        && (BYTE_CLASSES[bytes[self.position] as usize] & NAME_BYTE != 0)
                    {
                        self.position += 1;
                    }
                    let word = &self.source[start..self.position];
                    if word.len() <= 2
                        && word.bytes().all(|b| {
                            matches!(b, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b't' | b'T')
                        })
                        && matches!(bytes.get(self.position), Some(b'\'' | b'"'))
                    {
                        return Some(self.quoted(start, self.position));
                    }
                    match word {
                        "not" => Kind::Not,
                        "await" => Kind::Await,
                        "yield" => Kind::Yield,
                        "async" => Kind::Async,
                        "lambda" => Kind::Lambda,
                        "match" => Kind::Match,
                        "case" => Kind::Case,
                        "else" => Kind::Else,
                        "in" => Kind::In,
                        "is" => Kind::Is,
                        "and" => Kind::And,
                        "or" => Kind::Or,
                        "if" => Kind::If,
                        "elif" => Kind::Elif,
                        "for" => Kind::For,
                        _ => Kind::Name,
                    }
                }
                b'0'..=b'9' => {
                    // Keep numeric spelling opaque; resource accounting needs
                    // an operand token, not a decoded value or syntax check.
                    while self.position < bytes.len()
                        && (BYTE_CLASSES[bytes[self.position] as usize] & NUMBER_BYTE != 0)
                    {
                        self.position += 1;
                    }
                    Kind::Int
                }
                b'(' | b'[' | b'{' => {
                    if !self.push(Context::Bracket(match byte {
                        b'(' => b')',
                        b'[' => b']',
                        _ => b'}',
                    })) {
                        Kind::NestingLimit
                    } else {
                        match byte {
                            b'(' => Kind::Lpar,
                            b'[' => Kind::Lsqb,
                            _ => Kind::Lbrace,
                        }
                    }
                }
                b')' | b']' | b'}' => {
                    match context {
                        Context::Bracket(closer) if closer == byte => {
                            self.contexts.pop();
                        }
                        Context::Field if byte == b'}' => {
                            self.contexts.pop();
                        }
                        // Reject mismatches rather than losing the enclosing lexical context.
                        _ => {
                            self.stopped = true;
                            return Some(self.token(Kind::Limit, start));
                        }
                    }
                    match byte {
                        b')' => Kind::Rpar,
                        b']' => Kind::Rsqb,
                        _ => Kind::Rbrace,
                    }
                }
                b':' => {
                    if matches!(context, Context::Field) {
                        *self.contexts.last_mut().unwrap() = Context::Format;
                    }
                    Kind::Colon
                }
                b';' => Kind::Semi,
                b',' => Kind::Comma,
                b'+' => Kind::Plus,
                b'-' => Kind::Minus,
                b'~' => Kind::Tilde,
                b'%' => Kind::Percent,
                b'@' => Kind::At,
                b'|' => Kind::Vbar,
                b'&' => Kind::Amper,
                b'^' => Kind::CircumFlex,
                b'.' => Kind::Dot,
                b'*' | b'/' | b'<' | b'>' | b'=' | b'!' => {
                    let next = bytes.get(self.position).copied();
                    let pair = match (byte, next) {
                        (b'*', Some(b'*')) => Some(Kind::DoubleStar),
                        (b'/', Some(b'/')) => Some(Kind::DoubleSlash),
                        (b'<', Some(b'<')) => Some(Kind::LeftShift),
                        (b'>', Some(b'>')) => Some(Kind::RightShift),
                        (b'=', Some(b'=')) => Some(Kind::EqEqual),
                        (b'!', Some(b'=')) => Some(Kind::NotEqual),
                        (b'<', Some(b'=')) => Some(Kind::LessEqual),
                        (b'>', Some(b'=')) => Some(Kind::GreaterEqual),
                        _ => None,
                    };
                    if let Some(kind) = pair {
                        self.position += 1;
                        kind
                    } else {
                        match byte {
                            b'*' => Kind::Star,
                            b'/' => Kind::Slash,
                            b'<' => Kind::Less,
                            b'>' => Kind::Greater,
                            b'=' => Kind::Equal,
                            // Single `!` is used in f-string conversions. The
                            // parser still validates conversion syntax/placement.
                            _ => Kind::Other,
                        }
                    }
                }
                // Supported punctuation and non-ASCII name bytes are handled
                // above. Literal/comment contents bypass this code dispatch.
                _ => return Some(self.invalid_code_byte(start)),
            };
            return Some(self.token(kind, start));
        }
    }
}
