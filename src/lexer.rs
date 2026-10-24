//! Streaming byte lexer. It preserves source ranges and never decodes literals.
use crate::stack::{Mark, STRUCTURAL_CAPACITY, Stack};
use std::ops::Range;

#[cfg(feature = "experimental-regions")]
#[path = "lexer/region.rs"]
mod region;

const _: () = assert!(crate::MAX_SOURCE_BYTES <= u32::MAX as usize / 8);

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

// Keep nearby quotes and escapes cheap before using memchr's vectorized search.
// Delimiters are ASCII, so matches remain UTF-8 boundaries even when a search
// reads across multibyte characters.
#[inline(always)]
fn find_ascii<const N: usize>(text: &[u8], needles: [u8; N]) -> usize {
    let prefix = text.len().min(8);
    if let Some(offset) = text[..prefix]
        .iter()
        .position(|byte| needles.contains(byte))
    {
        return offset;
    }
    let tail = &text[prefix..];
    let found = match needles.as_slice() {
        [a, b] => memchr::memchr2(*a, *b, tail),
        [a, b, c] => memchr::memchr3(*a, *b, *c, tail),
        [a, b, c, d] => {
            // memchr searches up to three bytes. Bound both searches to a
            // constant-size block: repeated fourth-byte hits must not cause
            // rescanning of the entire remaining literal on every escape.
            let mut offset = 0;
            for chunk in tail.chunks(64) {
                let first = memchr::memchr3(*a, *b, *c, chunk).unwrap_or(chunk.len());
                if let Some(last) = memchr::memchr(*d, &chunk[..first]) {
                    return prefix + offset + last;
                }
                if first < chunk.len() {
                    return prefix + offset + first;
                }
                offset += chunk.len();
            }
            None
        }
        _ => unreachable!("literal searches use two, three, or four delimiters"),
    };
    prefix + found.unwrap_or(tail.len())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
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
    Match,
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

#[derive(Clone, Copy, Default)]
enum Context {
    #[default]
    Root,
    Bracket(u8),
    Text {
        quote: u8,
        // Python quote delimiters are one or three bytes wide.
        width: u8,
    },
    Field,
    Format,
}

#[derive(Default)]
pub(super) struct LexerScratch {
    contexts: Stack<Context, STRUCTURAL_CAPACITY>,
    indents: Stack<u32, STRUCTURAL_CAPACITY>,
}

#[derive(Clone, Copy)]
pub(super) struct LexerState {
    position: usize,
    line_start: bool,
    pending_indent: Option<usize>,
    stopped: bool,
}

impl LexerScratch {
    pub(super) fn with_capacity(contexts: usize, indents: usize) -> Self {
        Self {
            contexts: Stack::with_capacity(contexts),
            indents: Stack::with_capacity(indents),
        }
    }

    pub(super) fn suspend(&mut self) -> [Mark; 2] {
        [self.contexts.suspend(), self.indents.suspend()]
    }

    pub(super) fn restore(&mut self, saved: [Mark; 2]) {
        self.contexts.restore(saved[0]);
        self.indents.restore(saved[1]);
    }

    pub(super) fn clear_segments(&mut self) {
        self.contexts.reset();
        self.indents.reset();
    }
}

pub struct Lexer<'a, 's> {
    source: &'a str,
    position: usize,
    contexts: &'s mut Stack<Context, STRUCTURAL_CAPACITY>,
    indents: &'s mut Stack<u32, STRUCTURAL_CAPACITY>,
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

    pub(super) fn resume(
        source: &'a str,
        limit: usize,
        indentation_limit: usize,
        scratch: &'s mut LexerScratch,
        state: LexerState,
    ) -> Self {
        Self {
            source,
            position: state.position,
            contexts: &mut scratch.contexts,
            indents: &mut scratch.indents,
            line_start: state.line_start,
            pending_indent: state.pending_indent,
            limit,
            indentation_limit,
            stopped: state.stopped,
        }
    }

    pub(super) fn state(&self) -> LexerState {
        LexerState {
            position: self.position,
            line_start: self.line_start,
            pending_indent: self.pending_indent,
            stopped: self.stopped,
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

    #[cold]
    fn invalid_code_byte(&mut self, start: usize) -> Token {
        self.stopped = true;
        self.token(Kind::Limit, start)
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
    fn text_token(&mut self, context: Context) -> Option<Token> {
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
        None
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

        let previous = *self.indents.last().unwrap() as usize;
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
            // A tab adds at most eight columns per source byte; the hard
            // source-size ceiling therefore keeps indentation within u32.
            self.indents
                .push(u32::try_from(column).expect("indentation fits the hard source ceiling"));
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

#[cfg(test)]
mod tests {
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
            assert_eq!(
                super::find_ascii(text, needles),
                scalar,
                "{text:?}: {needles:?}"
            );
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
}
