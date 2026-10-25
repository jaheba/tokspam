//! Logical-line indentation, including tab expansion and deferred dedents.

use super::{Kind, Lexer, Token};

impl Lexer<'_, '_> {
    pub(super) fn indentation(&mut self) -> Option<Token> {
        let start = self.position;
        let column = if let Some(column) = self.pending_indent.take() {
            column
        } else {
            let mut column = 0usize;
            let bytes = self.source.as_bytes();
            while let Some(byte) = bytes.get(self.position) {
                match byte {
                    b' ' => column = column.saturating_add(1),
                    b'\t' => column = (column / 8).saturating_add(1).saturating_mul(8),
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

        // Emit one dedent per next() call. Keeping line_start set makes the
        // next call continue toward pending_indent rather than tokenize code.
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
            if !self.indents.push(column) {
                self.stopped = true;
                self.storage_failed = true;
                return Some(self.token(Kind::StorageLimit, start));
            }
            return Some(self.token(Kind::Indent, start));
        }
        None
    }
}
