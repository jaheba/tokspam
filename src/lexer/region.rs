//! Statement proofs sharing the detailed lexer's workspace.
use super::{BYTE_CLASSES, Context, Kind, Lexer, NAME_BYTE, NUMBER_BYTE};

impl Lexer<'_, '_> {
    /// Proves a complete logical statement, committing only its final newline.
    /// Failed attempts leave all detailed lexer state intact for local fallback.
    pub(crate) fn skip_region(
        &mut self,
        max_depth: usize,
        max_tokens: usize,
        max_token_bytes: Option<usize>,
        check_annotations: bool,
        annotation_source: bool,
    ) -> Option<(usize, bool)> {
        if self.stopped || self.pending_indent.is_some() || self.contexts.len() != 1 {
            return None;
        }
        let start = self.position;
        // Region tokens all consume source bytes: there are no synthetic
        // layout tokens here. A byte bound can eliminate per-token budget
        // comparisons while still retaining an exact count for later regions.
        let result = if max_tokens >= self.source.len() - start {
            self.prove_region::<false>(
                max_depth,
                max_tokens,
                max_token_bytes,
                check_annotations,
                annotation_source,
            )
        } else {
            self.prove_region::<true>(
                max_depth,
                max_tokens,
                max_token_bytes,
                check_annotations,
                annotation_source,
            )
        };

        // Proofs use the existing context stack and literal scanner. In
        // particular, malformed literals may set stopped or move position.
        self.contexts.truncate(1);
        self.position = start;
        self.stopped = false;
        let (end, tokens, seen_code) = result?;
        self.position = end;
        self.line_start = true;
        Some((tokens, seen_code))
    }

    fn prove_region<const COUNT_LIMIT: bool>(
        &mut self,
        max_depth: usize,
        max_tokens: usize,
        max_token_bytes: Option<usize>,
        check_annotations: bool,
        annotation_source: bool,
    ) -> Option<(usize, usize, bool)> {
        let bytes = self.source.as_bytes();
        let mut position = self.position;
        if self.line_start {
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
            // Layout changes must first pass through the detailed lexer, so
            // the structural scanner's indentation state stays exact.
            if column != *self.indents.last().unwrap()
                && !matches!(bytes.get(position), None | Some(b'#' | b'\n' | b'\r'))
            {
                return None;
            }
        }

        let mut tokens = 0;
        let mut weight = 0;
        let mut possible_annotation = annotation_source;
        let mut seen_code = false;
        while position < bytes.len() {
            let start = position;
            let byte = bytes[position];
            position += 1;
            let mut finished = false;
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
                        return None;
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
                    if self.contexts.len() != 1 {
                        continue;
                    }
                    finished = true;
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' | 0x80..=0xff => {
                    while position < bytes.len()
                        && BYTE_CLASSES[bytes[position] as usize] & NAME_BYTE != 0
                    {
                        position += 1;
                    }
                    let word = &bytes[start..position];
                    if word == b"lambda" || !seen_code && matches!(word, b"elif" | b"else") {
                        // Pending lambdas and branch runs can survive the
                        // newline. Their state belongs to detailed scanning.
                        return None;
                    }
                    if word.len() <= 2
                        && word.iter().all(|b| {
                            matches!(b, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b't' | b'T')
                        })
                        && matches!(bytes.get(position), Some(b'\'' | b'"'))
                    {
                        if word.iter().any(|b| matches!(b, b'f' | b'F' | b't' | b'T')) {
                            return None;
                        }
                        position = self.region_literal(
                            start,
                            position,
                            max_depth.saturating_sub(weight),
                            check_annotations && possible_annotation,
                        )?;
                    } else {
                        weight += match word {
                            b"not" | b"await" | b"yield" | b"async" => 1,
                            b"else" => 2,
                            _ => 0,
                        };
                    }
                }
                b'0'..=b'9' => {
                    while position < bytes.len()
                        && BYTE_CLASSES[bytes[position] as usize] & NUMBER_BYTE != 0
                    {
                        position += 1;
                    }
                }
                b'\'' | b'"' => {
                    position = self.region_literal(
                        start,
                        start,
                        max_depth.saturating_sub(weight),
                        check_annotations && possible_annotation,
                    )?;
                }
                b'(' | b'[' | b'{' => {
                    if !self.push(Context::Bracket(match byte {
                        b'(' => b')',
                        b'[' => b']',
                        _ => b'}',
                    })) {
                        return None;
                    }
                    // A call/subscript can retain an AST operand in addition
                    // to the shared bracket frame. Grouping is overcounted.
                    weight += if byte == b'{' { 1 } else { 2 };
                }
                b')' | b']' | b'}' => {
                    if self.contexts.len() == 1 {
                        return None;
                    }
                    if !matches!(self.contexts.pop(), Some(Context::Bracket(expected)) if expected == byte)
                    {
                        return None;
                    }
                }
                b':' => possible_annotation = true,
                b';' => return None,
                b'-' => {
                    possible_annotation |= bytes.get(position) == Some(&b'>');
                    weight += 1;
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
                    weight += 1;
                }
                b'+' | b'~' | b'%' | b'@' | b'|' | b'&' | b'^' | b'.' => weight += 1,
                b',' => {}
                _ => return None,
            }
            tokens += 1;
            if COUNT_LIMIT && tokens > max_tokens
                || weight > max_depth
                || max_token_bytes.is_some_and(|limit| position - start > limit)
            {
                return None;
            }
            if finished {
                return Some((position, tokens, seen_code));
            }
            seen_code = true;
        }
        None
    }

    fn region_literal(
        &mut self,
        start: usize,
        quote_start: usize,
        _remaining: usize,
        annotation: bool,
    ) -> Option<usize> {
        let token = self.quoted(start, quote_start);
        // A proof cannot account for child token/work or annotation-depth
        // scores. Leave type strings to the detailed scan even when short.
        if token.kind != Kind::String || annotation && token.contents.is_some() {
            return None;
        }
        Some(token.range.end)
    }
}
