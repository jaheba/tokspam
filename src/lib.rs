//! Experimental pre-parse screening with an independent lexer.
//! Acceptance is not yet a verified bound on Python parser recursion.
//!
//! ```
//! use spam::{Limits, check};
//! let limits = Limits { max_nesting_depth: 80, ..Limits::default() };
//! assert!(check("x = [1, 2, 3]", &limits).is_ok());
//! ```
mod lexer;
mod rules;

use lexer::{Kind, Lexer};
use rules::Scanner;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    SourceSize,
    TokenCount,
    TokenSize,
    Nesting,
    Indentation,
    AstDepth,
    LexicalStructure,
    QuotedAnnotation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub reason: Reason,
    pub range: Range<usize>,
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} at bytes {}..{}",
            self.reason, self.range.start, self.range.end
        )
    }
}

impl std::error::Error for Rejection {}

/// Screening budgets. These are heuristic limits, not parser safety guarantees.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_source_bytes: usize,
    pub max_tokens: usize,
    pub max_token_bytes: Option<usize>,
    pub max_nesting_depth: u16,
    pub max_indentation_depth: u16,
    pub max_annotation_depth: u8,
    pub check_string_annotations: bool,
    /// Flat AST-shape estimates supplement the parser-recursion rules.
    pub max_ast_depth: Option<u16>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            max_tokens: 100_000,
            max_token_bytes: None,
            max_nesting_depth: 100,
            max_indentation_depth: 80,
            max_annotation_depth: 8,
            check_string_annotations: true,
            max_ast_depth: Some(100),
        }
    }
}

/// Rejects source that exceeds the configured structural budgets.
/// Successful screening does not validate Python syntax.
pub fn check(source: &str, limits: &Limits) -> Result<(), Rejection> {
    if source.len() > limits.max_source_bytes {
        return Err(Rejection {
            reason: Reason::SourceSize,
            range: 0..source.len(),
        });
    }
    scan(
        source,
        limits,
        u32::from(limits.max_nesting_depth),
        limits.max_ast_depth.map(u32::from),
        0,
        false,
    )
}

fn scan(
    source: &str,
    limits: &Limits,
    budget: u32,
    ast_budget: Option<u32>,
    string_depth: u8,
    annotation_source: bool,
) -> Result<(), Rejection> {
    let mut scanner = Scanner::new(budget, false);
    let mut ast = ast_budget.map(|budget| Scanner::new(budget, true));
    let mut annotations = AnnotationTracker::default();
    let mut token_count = 0;
    for token in Lexer::new(
        source,
        budget as usize,
        usize::from(limits.max_indentation_depth),
    ) {
        token_count += 1;
        if token_count > limits.max_tokens {
            return Err(Rejection {
                reason: Reason::TokenCount,
                range: token.range,
            });
        }
        if limits
            .max_token_bytes
            .is_some_and(|limit| token.range.len() > limit)
        {
            return Err(Rejection {
                reason: Reason::TokenSize,
                range: token.range,
            });
        }
        let reason = match token.kind {
            Kind::Limit => Some(Reason::LexicalStructure),
            Kind::NestingLimit => Some(Reason::Nesting),
            Kind::IndentationLimit => Some(Reason::Indentation),
            _ => None,
        };
        if let Some(reason) = reason {
            return Err(Rejection {
                reason,
                range: token.range,
            });
        }
        if limits.check_string_annotations && (annotation_source || annotations.active()) {
            if let Some(contents) = token.contents {
                // Raw contents avoid allocating decoded strings. Escaped annotations
                // need separate rules before this can model their parsed depth.
                let remaining = budget.saturating_sub(scanner.total);
                let ast_remaining = ast
                    .as_ref()
                    .zip(ast_budget)
                    .map(|(scanner, budget)| budget.saturating_sub(scanner.total));
                let minimum = remaining.min(ast_remaining.unwrap_or(remaining));
                if contents.len() > minimum as usize
                    && (string_depth >= limits.max_annotation_depth
                        || scan(
                            &source[contents],
                            limits,
                            remaining,
                            ast_remaining,
                            string_depth + 1,
                            true,
                        )
                        .is_err())
                {
                    return Err(Rejection {
                        reason: Reason::QuotedAnnotation,
                        range: token.range,
                    });
                }
            }
        }
        if scanner.observe(token.kind) {
            return Err(Rejection {
                reason: Reason::Nesting,
                range: token.range,
            });
        }
        if ast
            .as_mut()
            .is_some_and(|scanner| scanner.observe(token.kind))
        {
            return Err(Rejection {
                reason: Reason::AstDepth,
                range: token.range,
            });
        }
        annotations.observe(source, token.kind, &token.range);
    }
    Ok(())
}

#[derive(Default)]
struct AnnotationTracker {
    brackets: Vec<Kind>,
    start_depth: Option<usize>,
    line_started: bool,
    block_header: bool,
    pending_lambdas: u32,
    previous: Option<(Kind, usize)>,
}

impl AnnotationTracker {
    fn active(&self) -> bool {
        self.start_depth.is_some()
    }

    fn observe(&mut self, source: &str, kind: Kind, range: &Range<usize>) {
        if matches!(kind, Kind::Newline | Kind::Semi) {
            self.start_depth = None;
            self.line_started = false;
            self.block_header = false;
            self.previous = None;
            return;
        }
        if !self.line_started && !matches!(kind, Kind::Indent | Kind::Dedent) {
            self.line_started = true;
            self.block_header = matches!(
                &source[range.clone()],
                "if" | "elif"
                    | "else"
                    | "while"
                    | "for"
                    | "async"
                    | "with"
                    | "try"
                    | "except"
                    | "finally"
                    | "def"
                    | "class"
                    | "match"
                    | "case"
            );
        }
        if matches!(kind, Kind::Equal | Kind::Comma)
            && self.start_depth == Some(self.brackets.len())
        {
            self.start_depth = None;
        }
        if matches!(kind, Kind::Rpar | Kind::Rsqb | Kind::Rbrace)
            && self
                .start_depth
                .is_some_and(|depth| self.brackets.len() <= depth)
        {
            self.start_depth = None;
        }
        if kind == Kind::Lambda {
            self.pending_lambdas += 1;
        }
        if kind == Kind::Colon {
            if self.pending_lambdas > 0 {
                self.pending_lambdas -= 1;
            } else if !matches!(self.brackets.last(), Some(Kind::Lbrace | Kind::Lsqb))
                && (!self.brackets.is_empty() || !self.block_header)
            {
                self.start_depth = Some(self.brackets.len());
            }
        }
        if kind == Kind::Greater && self.previous == Some((Kind::Minus, range.start)) {
            self.start_depth = Some(self.brackets.len());
        }
        match kind {
            Kind::Lpar | Kind::Lsqb | Kind::Lbrace => self.brackets.push(kind),
            Kind::Rpar | Kind::Rsqb | Kind::Rbrace => {
                self.brackets.pop();
            }
            _ => {}
        }
        self.previous = Some((kind, range.end));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str) -> Result<(), Rejection> {
        super::check(
            source,
            &Limits {
                max_source_bytes: 1_000_000,
                max_nesting_depth: 30,
                max_ast_depth: None,
                ..Limits::default()
            },
        )
    }

    #[test]
    fn fstrings_and_templates() {
        for source in [
            "f'{x}'",
            "rf'{x!r:>10}'",
            "t'{x}'",
            "f'{{literal}} {a[0]:{width}.{precision}f}'",
            "f\"{f'{x}'}\"",
            "f'{\"same quote\"}'",
            "f'{ {\"key\": value} }'",
            "f'{x:{{}}}'",
        ] {
            assert!(check(source).is_ok(), "{source}");
        }
        assert!(check(&format!("f'{}{{x}}{}'", "{x:".repeat(50), "}".repeat(50))).is_err());
        assert!(check(&format!("f'{{ {}1 }}'", "-".repeat(50))).is_err());
        assert!(check(&format!("f'{}'", "literal {{text}} ".repeat(1_000))).is_ok());
    }

    #[test]
    fn indentation_and_continuations() {
        let deep: String = (0..40)
            .map(|i| format!("{}if x:\n", " ".repeat(i)))
            .collect();
        assert!(check(&deep).is_err());
        assert!(check(&"if x:\n\tpass\nelse:\n\tpass\n".repeat(100)).is_ok());
        assert!(check(&format!("x = (\n{}1\n)\n", " ".repeat(1000))).is_ok());
        assert!(check(&format!("x = {}1", "-\\\n".repeat(50))).is_err());
        assert!(check("if x:\r\n\tpass\r\n# comment\r\nx = 1\r\n").is_ok());
    }

    #[test]
    fn quoted_annotations_and_bytes() {
        for quote in ["'", "\"", "'''", "\"\"\""] {
            assert!(
                check(&format!(
                    "x: {quote}{}int{}{quote}",
                    "[".repeat(50),
                    "]".repeat(50)
                ))
                .is_err()
            );
        }
        assert!(check(&format!("x = b'{}'", "(".repeat(100))).is_ok());
        assert!(check("x: \"list['int']\"").is_ok());
        assert!(check("x: \"\"\"'''\"'int'\"'''\"\"\"").is_ok());
    }

    #[test]
    fn malformed_and_unicode() {
        for source in [
            "π = '🦀'",
            "'unterminated\\",
            "\"\"\"",
            "\\\n",
            "\0🦀",
            "f'{",
            "([)]",
        ] {
            for token in Lexer::new(source, 30, 30) {
                assert!(source.is_char_boundary(token.range.start));
                assert!(source.is_char_boundary(token.range.end));
            }
        }
        // An unterminated short string cannot hide code on following lines.
        assert!(check(&format!("'broken\n{}1", "-".repeat(50))).is_err());
    }

    #[test]
    fn seeded_malformed_sources_terminate() {
        let alphabet = [
            'a', 'π', '🦀', '0', 'f', 'r', '\'', '"', '{', '}', '(', ')', '[', ']', ':', '\\',
            '\n', '\r', '\t', '#', '-', '*',
        ];
        let mut state = 42u64;
        for _ in 0..2_000 {
            let mut source = String::new();
            for _ in 0..100 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                source.push(alphabet[(state >> 32) as usize % alphabet.len()]);
            }
            let tokens: Vec<_> = Lexer::new(&source, 30, 30)
                .take(source.len() * 2 + 32)
                .collect();
            assert!(tokens.len() < source.len() * 2 + 32);
            for token in tokens {
                assert!(source.is_char_boundary(token.range.start));
                assert!(source.is_char_boundary(token.range.end));
            }
            let _ = check(&source);
        }
    }

    #[test]
    fn ordinary_flat_code() {
        assert!(check(&"x = [-1, -2]\n".repeat(100)).is_ok());
        assert!(check(&format!("x = [{}]", "-1, ".repeat(3000))).is_ok());
        assert!(check(&format!("x = {{{}}}", "'k': lambda a, b: 0, ".repeat(500))).is_ok());
    }
}
