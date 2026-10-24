//! Experimental acceptance-only byte guard for the existing Spam checker.
//! Inconclusive input must fall back to `Checker`; this is not a parser guarantee.
use spam::Limits;

pub struct ByteGuard {
    limits: Limits,
    brackets: Vec<u8>,
    indents: Vec<usize>,
}

impl ByteGuard {
    pub fn new(limits: Limits) -> Self {
        Self {
            brackets: Vec::with_capacity(usize::from(limits.max_nesting_depth)),
            indents: Vec::with_capacity(usize::from(limits.max_indentation_depth) + 1),
            limits,
        }
    }

    /// Returns true only when conservative bounds prove that Spam accepts.
    /// False includes both rejected and accepted inputs outside this prototype.
    pub fn proves_acceptance(&mut self, source: &str) -> bool {
        let limits = &self.limits;
        // Keep the experiment within the default source cap and avoid modeling
        // optional per-token ranges before its coverage and speed are measured.
        if source.len() > limits.max_source_bytes.min(1024 * 1024)
            || limits.max_token_bytes.is_some()
        {
            return false;
        }
        self.brackets.clear();
        self.indents.clear();
        self.indents.push(0);

        let bytes = source.as_bytes();
        let mut position = 0;
        let mut line_start = true;
        let mut tokens = 0usize;
        let mut weight = 0u32;
        let mut elifs = 0u32;
        let mut possible_annotation = false;

        while position < bytes.len() {
            if line_start {
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
                if !matches!(bytes.get(position), None | Some(b'#' | b'\n' | b'\r')) {
                    while column < *self.indents.last().unwrap() {
                        self.indents.pop();
                        tokens += 1;
                    }
                    if column > *self.indents.last().unwrap() {
                        if self.indents.len() > usize::from(limits.max_indentation_depth) {
                            return false;
                        }
                        self.indents.push(column);
                        tokens += 1;
                    }
                }
                let depth = (self.indents.len() - 1) as u32 + weight;
                if tokens > limits.max_tokens
                    || depth > u32::from(limits.max_nesting_depth)
                    || limits
                        .max_ast_depth
                        .is_some_and(|limit| depth + elifs > u32::from(limit))
                {
                    return false;
                }
                line_start = false;
                if position == bytes.len() {
                    break;
                }
            }

            let start = position;
            let byte = bytes[position];
            position += 1;
            match byte {
                b' ' | b'\t' | 0x0c => continue,
                b'#' => {
                    while position < bytes.len() && !matches!(bytes[position], b'\n' | b'\r') {
                        position += 1;
                    }
                    continue;
                }
                b'\\' => {
                    if !matches!(bytes.get(position), Some(b'\n' | b'\r')) {
                        return false;
                    }
                    if bytes[position] == b'\r' {
                        position += 1;
                    }
                    if bytes.get(position) == Some(&b'\n') {
                        position += 1;
                    }
                    continue;
                }
                b'\n' | b'\r' => {
                    if byte == b'\r' && bytes.get(position) == Some(&b'\n') {
                        position += 1;
                    }
                    if !self.brackets.is_empty() {
                        continue;
                    }
                    tokens += 1;
                    line_start = true;
                    weight = 0;
                    possible_annotation = false;
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' | 0x80..=0xff => {
                    while position < bytes.len()
                        && (bytes[position].is_ascii_alphanumeric()
                            || bytes[position] == b'_'
                            || bytes[position] >= 128)
                    {
                        position += 1;
                    }
                    let word = &bytes[start..position];
                    if word.len() <= 2
                        && word.iter().all(|b| {
                            matches!(b, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b't' | b'T')
                        })
                        && matches!(bytes.get(position), Some(b'\'' | b'"'))
                        && word.iter().any(|b| matches!(b, b'f' | b'F' | b't' | b'T'))
                    {
                        // Interpolation has its own lexical contexts. Keep its
                        // complete handling in the existing lexer for now.
                        return false;
                    }
                    match word {
                        b"not" | b"await" | b"yield" | b"async" => weight += 1,
                        b"else" => weight += 2,
                        b"elif" => elifs += 1,
                        _ => {}
                    }
                    // A string prefix is overcounted as a separate name. This
                    // can only force fallback sooner; it cannot bypass a limit.
                    tokens += 1;
                }
                b'0'..=b'9' => {
                    while position < bytes.len()
                        && (bytes[position].is_ascii_alphanumeric()
                            || matches!(bytes[position], b'_' | b'.'))
                    {
                        position += 1;
                    }
                    tokens += 1;
                }
                b'\'' | b'"' => {
                    let Some((end, contents)) = literal_end(source, start) else {
                        return false;
                    };
                    let depth = (self.indents.len() - 1) as u32 + weight;
                    let remaining = u32::from(limits.max_nesting_depth)
                        .saturating_sub(depth)
                        .min(limits.max_ast_depth.map_or(u32::MAX, |limit| {
                            u32::from(limit).saturating_sub(depth + elifs)
                        }));
                    if limits.check_string_annotations
                        && possible_annotation
                        && contents > remaining as usize
                    {
                        return false;
                    }
                    position = end;
                    tokens += 1;
                }
                b'(' | b'[' | b'{' => {
                    if self.brackets.len() >= usize::from(limits.max_nesting_depth) {
                        return false;
                    }
                    self.brackets.push(match byte {
                        b'(' => b')',
                        b'[' => b']',
                        _ => b'}',
                    });
                    // Calls/subscripts can add an AST frame as well as a
                    // bracket frame. Count both even for plain grouping.
                    weight += if byte == b'{' { 1 } else { 2 };
                    tokens += 1;
                }
                b')' | b']' | b'}' => {
                    if self.brackets.pop() != Some(byte) {
                        return false;
                    }
                    tokens += 1;
                }
                b':' => {
                    // A pending lambda can open two frames at a later colon,
                    // even across malformed lines. Charge every colon twice.
                    weight += 2;
                    possible_annotation = true;
                    tokens += 1;
                }
                b'-' => {
                    possible_annotation |= bytes.get(position) == Some(&b'>');
                    weight += 1;
                    tokens += 1;
                }
                b'+' | b'*' | b'/' | b'~' | b'%' | b'@' | b'|' | b'&' | b'^' | b'.' | b'<'
                | b'>' | b'=' | b'!' => {
                    // Counting operator bytes separately overcounts paired
                    // operators and gives an upper bound on token count.
                    weight += 1;
                    tokens += 1;
                }
                b',' | b';' => tokens += 1,
                _ => return false,
            }

            let depth = (self.indents.len() - 1) as u32 + weight;
            if tokens > limits.max_tokens
                || depth > u32::from(limits.max_nesting_depth)
                || limits
                    .max_ast_depth
                    .is_some_and(|limit| depth + elifs > u32::from(limit))
            {
                return false;
            }
        }

        self.brackets.is_empty() && tokens <= limits.max_tokens
    }
}

fn literal_end(source: &str, start: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let quote = bytes[start];
    let width = if bytes.get(start..start + 3) == Some(&[quote; 3]) {
        3
    } else {
        1
    };
    let inner = start + width;
    let mut position = inner;

    while position < bytes.len() {
        let text = &bytes[position..];
        let special = if width == 1 {
            text.iter()
                .position(|&byte| byte == quote || matches!(byte, b'\\' | b'\n' | b'\r'))
        } else {
            text.iter().position(|&byte| byte == quote || byte == b'\\')
        };
        position += special?;
        let byte = bytes[position];
        if byte == quote && (width == 1 || bytes.get(position..position + 3) == Some(&[quote; 3])) {
            return Some((position + width, position - inner));
        }
        if width == 1 && matches!(byte, b'\n' | b'\r') {
            return None;
        }
        position += 1;
        if byte == b'\\' && position < bytes.len() {
            position += source[position..].chars().next()?.len_utf8();
        }
    }
    None
}
