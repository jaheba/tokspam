//! Streaming byte lexer. It preserves source ranges and never decodes literals.
use std::ops::Range;

#[cfg(feature = "experimental-regions")]
mod region;

const NAME_BYTE: u8 = 1;
const NUMBER_BYTE: u8 = 2;

// One lookup classifies continuation bytes in the hot identifier/number loops.
// Non-ASCII bytes are kept together as names, matching the streaming lexer.
const BYTE_CLASSES: [u8; 256] = {
    let mut classes = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let ascii_word = (byte >= b'a' as usize && byte <= b'z' as usize)
            || (byte >= b'A' as usize && byte <= b'Z' as usize)
            || (byte >= b'0' as usize && byte <= b'9' as usize)
            || byte == b'_' as usize;
        if ascii_word || byte >= 128 {
            classes[byte] |= NAME_BYTE;
        }
        if ascii_word || byte == b'.' as usize {
            classes[byte] |= NUMBER_BYTE;
        }
        byte += 1;
    }
    classes
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Name,
    Int,
    String,
    FStringStart,
    FStringEnd,
    Lpar,
    Lsqb,
    Lbrace,
    Rpar,
    Rsqb,
    Rbrace,
    Indent,
    Dedent,
    Newline,
    Semi,
    Minus,
    Plus,
    Star,
    Not,
    Tilde,
    Await,
    Yield,
    Async,
    DoubleStar,
    Lambda,
    Case,
    Colon,
    Else,
    Comma,
    Slash,
    DoubleSlash,
    Percent,
    At,
    Vbar,
    Amper,
    CircumFlex,
    LeftShift,
    RightShift,
    Less,
    Greater,
    EqEqual,
    NotEqual,
    LessEqual,
    GreaterEqual,
    In,
    Is,
    And,
    Or,
    If,
    Elif,
    For,
    Dot,
    Equal,
    Other,
    Limit,
    NestingLimit,
    IndentationLimit,
}

#[derive(Debug)]
pub struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
    pub contents: Option<Range<usize>>,
}

#[derive(Clone, Copy)]
enum Context {
    Root,
    Bracket(u8),
    Text { quote: u8, width: usize },
    Field,
    Format,
}

pub(super) struct LexerScratch {
    contexts: Vec<Context>,
    indents: Vec<usize>,
}

impl LexerScratch {
    pub(super) fn new(limit: usize, indentation_limit: usize) -> Self {
        Self {
            contexts: Vec::with_capacity(limit + 1),
            indents: Vec::with_capacity(indentation_limit + 1),
        }
    }
}

pub struct Lexer<'a, 's> {
    source: &'a str,
    position: usize,
    contexts: &'s mut Vec<Context>,
    indents: &'s mut Vec<usize>,
    line_start: bool,
    pending_indent: Option<usize>,
    limit: usize,
    indentation_limit: usize,
    stopped: bool,
}

impl<'a, 's> Lexer<'a, 's> {
    pub fn new(
        source: &'a str,
        limit: usize,
        indentation_limit: usize,
        scratch: &'s mut LexerScratch,
    ) -> Self {
        scratch.contexts.clear();
        scratch.contexts.push(Context::Root);
        scratch.indents.clear();
        scratch.indents.push(0);

        Self {
            source,
            position: 0,
            contexts: &mut scratch.contexts,
            indents: &mut scratch.indents,
            line_start: true,
            pending_indent: None,
            limit,
            indentation_limit,
            stopped: false,
        }
    }

    // Only an unindented prefix of standalone ASCII lines qualifies. Strings,
    // delimiters, annotations and continuations use the detailed lexer instead.
    // This routine commits its position only after every bound has passed.
    pub(super) fn skip_simple_line(
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

    fn token(&self, kind: Kind, start: usize) -> Token {
        Token {
            kind,
            range: start..self.position,
            contents: None,
        }
    }

    fn push(&mut self, context: Context) -> bool {
        if self.contexts.len() > self.limit {
            self.stopped = true;
            false
        } else {
            self.contexts.push(context);
            true
        }
    }

    fn quoted(&mut self, start: usize, quote_start: usize) -> Token {
        let bytes = self.source.as_bytes();
        let quote = bytes[quote_start];
        let width = if bytes.get(quote_start..quote_start + 3) == Some(&[quote; 3]) {
            3
        } else {
            1
        };
        self.position = quote_start + width;
        let prefix = &bytes[start..quote_start];
        if prefix
            .iter()
            .any(|b| matches!(b, b'f' | b'F' | b't' | b'T'))
        {
            let kind = if self.push(Context::Text { quote, width }) {
                Kind::FStringStart
            } else {
                Kind::NestingLimit
            };
            return self.token(kind, start);
        }

        let inner = self.position;
        while self.position < bytes.len() {
            // Search literal text in a tight byte loop; only quote candidates
            // need the more expensive triple-quote delimiter comparison.
            let text = &bytes[self.position..];
            let special = if width == 1 {
                text.iter()
                    .position(|&byte| byte == quote || matches!(byte, b'\\' | b'\n' | b'\r'))
            } else {
                text.iter().position(|&byte| byte == quote || byte == b'\\')
            };
            let Some(offset) = special else {
                self.position = bytes.len();
                break;
            };
            self.position += offset;
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

    fn indentation(&mut self) -> Option<Token> {
        let start = self.position;
        let column = if let Some(column) = self.pending_indent.take() {
            column
        } else {
            let mut column = 0;
            let bytes = self.source.as_bytes();
            while let Some(byte) = bytes.get(self.position) {
                match byte {
                    b' ' => column += 1,
                    b'\t' => column = (column / 8 + 1) * 8,
                    0x0c => column = 0,
                    _ => break,
                }
                self.position += 1;
            }
            if matches!(bytes.get(self.position), None | Some(b'#' | b'\n' | b'\r')) {
                self.line_start = false;
                return None;
            }
            column
        };

        let previous = *self.indents.last().unwrap();
        if column < previous {
            self.indents.pop();
            self.pending_indent = Some(column);
            return Some(self.token(Kind::Dedent, start));
        }
        self.line_start = false;
        if column > previous {
            if self.indents.len() > self.indentation_limit {
                self.stopped = true;
                return Some(self.token(Kind::IndentationLimit, start));
            }
            self.indents.push(column);
            return Some(self.token(Kind::Indent, start));
        }
        None
    }
}

impl Iterator for Lexer<'_, '_> {
    type Item = Token;

    #[inline(always)]
    fn next(&mut self) -> Option<Token> {
        if self.stopped {
            return None;
        }
        let bytes = self.source.as_bytes();
        loop {
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

            if let Context::Text { quote, width } = context {
                if bytes.get(start..start + width) == Some(&[quote; 3][..width]) {
                    self.position += width;
                    self.contexts.pop();
                    return Some(self.token(Kind::FStringEnd, start));
                }
            }
            if matches!(context, Context::Text { .. } | Context::Format) {
                if byte == b'{' {
                    if matches!(context, Context::Text { .. })
                        && bytes.get(start + 1) == Some(&b'{')
                    {
                        self.position += 2;
                        continue;
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
                    continue;
                }
                // Backslashes cannot hide replacement-field braces, including \N{...}.
                if byte == b'\\'
                    && bytes
                        .get(start + 1)
                        .is_some_and(|b| matches!(b, b'\\' | b'\'' | b'"'))
                {
                    self.position += 2;
                } else {
                    self.position += self.source[start..].chars().next().unwrap().len_utf8();
                }
                continue;
            }

            self.position += 1;
            let kind = match byte {
                b' ' | b'\t' | 0x0c => continue,
                b'#' => {
                    while self.position < bytes.len()
                        && !matches!(bytes[self.position], b'\n' | b'\r')
                    {
                        self.position += 1;
                    }
                    continue;
                }
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
                            _ => Kind::Other,
                        }
                    }
                }
                _ => Kind::Other,
            };
            return Some(self.token(kind, start));
        }
    }
}
