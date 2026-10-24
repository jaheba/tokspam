//! Recursion rules adapted from Monty's existing source-nesting guard.
use super::Reason;
use super::lexer::Kind;

/// Estimates parser frames from lexical structure.
/// Brackets, operators, bodies, format specs and blocks consume depth.
/// Coverage of malformed input remains experimental.
/// Parser and AST estimates share structural state and keep separate totals.
pub(super) struct Scanner {
    limit: u32,
    ast_limit: Option<u32>,
    pub(super) ast_total: u32,
    /// Open bracket levels, bottom entry being the module or expression itself.
    levels: Vec<Level>,
    /// Open indented blocks.
    indent: u32,
    /// Whether the previous token can end an operand, making a following
    /// `-`, `+`, `*` or `not` binary rather than prefix.
    prev_ends_operand: bool,
    /// Whether the next non-layout token starts a logical statement.
    statement_start: bool,
    /// Whether `case` began the current logical line. Complex-literal `+` and
    /// `-` patterns recurse on each right operand in Ruff's parser.
    in_pattern: bool,
    /// Whether the next indent opens a `match` body.
    pending_match: bool,
    /// True for indentation levels whose suite contains `case` clauses.
    match_blocks: Vec<bool>,
    /// Consecutive `elif` clauses at each indentation level. Python stores
    /// these as nested `If` nodes despite their flat source indentation.
    elif_runs: Vec<u32>,
    /// Open levels (excluding the bottom), indentation and every level's counts.
    pub(super) total: u32,
}

/// Frames open at one bracket level.
struct Level {
    kind: LevelKind,
    /// Prefix operators and `**` right operands: any lower-precedence binary
    /// operator proves they have returned.
    tight: u32,
    /// Lambda bodies, `else` branches and format specs: only the level's end
    /// or a comma, newline or statement colon proves they have returned.
    loose: u32,
    /// `lambda` keywords whose `:` has not arrived, so commas are still
    /// separating parameters rather than ending a body.
    pending_lambdas: u32,
    // Strictly increasing precedences in 1..=6 need at most six entries.
    binary_stack: [(u8, u32); 6],
    binary_len: usize,
    binary_depth: u32,
    ast_loose: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LevelKind {
    /// The module or expression itself.
    Top,
    /// `(`, `[` or `{` outside an f-string.
    Bracket,
    /// Between `FStringStart` and `FStringEnd` (or the t-string pair).
    FString,
    /// A `{...}` replacement field; `in_spec` once its format spec began,
    /// where a nested `{` opens another replacement field.
    Interpolation { in_spec: bool },
}

impl Scanner {
    pub(super) fn new(limit: u32, ast_limit: Option<u32>) -> Self {
        // Include the root and one overflowing entry: observe reports the
        // rejection after pushing the token that exceeds the budget.
        let mut scanner = Self {
            limit,
            ast_limit,
            ast_total: 0,
            levels: Vec::with_capacity(limit as usize + 2),
            indent: 0,
            prev_ends_operand: false,
            statement_start: true,
            in_pattern: false,
            pending_match: false,
            match_blocks: Vec::with_capacity(limit as usize + 2),
            elif_runs: Vec::with_capacity(limit as usize + 2),
            total: 0,
        };
        scanner.reset(limit, ast_limit);
        scanner
    }

    pub(super) fn reset(&mut self, limit: u32, ast_limit: Option<u32>) {
        self.limit = limit;
        self.ast_limit = ast_limit;
        self.ast_total = 0;
        self.levels.clear();
        self.levels.push(Level::new(LevelKind::Top));
        self.elif_runs.clear();
        self.elif_runs.push(0);
        self.indent = 0;
        self.prev_ends_operand = false;
        self.statement_start = true;
        self.in_pattern = false;
        self.pending_match = false;
        self.match_blocks.clear();
        self.match_blocks.push(false);
        self.total = 0;
    }

    #[cfg(feature = "experimental-regions")]
    pub(super) fn region_budget(&self) -> Option<usize> {
        // Called only after a logical newline/layout token. These boundaries
        // release local frames; only unfinished lambda parameters can survive.
        debug_assert!(self.statement_start && self.levels.len() == 1);
        let top = &self.levels[0];
        debug_assert!(
            top.tight == 0 && top.loose == 0 && top.ast_loose == 0 && top.binary_depth == 0
        );
        if top.pending_lambdas != 0 {
            return None;
        }
        let remaining = self.limit.saturating_sub(self.total);
        Some(
            remaining.min(
                self.ast_limit
                    .map_or(remaining, |limit| limit.saturating_sub(self.ast_total)),
            ) as usize,
        )
    }

    #[cfg(feature = "experimental-regions")]
    pub(super) fn finish_region(&mut self, seen_code: bool) {
        // First-token elif/else and pending lambdas are excluded by the proof.
        // A normal first token clears this indent's branch run, then newline
        // leaves all local counters empty while ancestor runs remain retained.
        if seen_code && self.ast_limit.is_some() {
            self.observe_statement_start(Kind::Name);
        }
        self.prev_ends_operand = false;
        self.statement_start = true;
    }

    /// Updates both estimates, preserving parser rejection priority on a token.
    #[inline]
    pub(super) fn observe(&mut self, kind: Kind) -> Option<Reason> {
        let starts_statement = self.statement_start;
        if self.ast_limit.is_some() {
            self.observe_statement_start(kind);

            // Calls, subscripts and attributes retain their left operand in the
            // AST after the corresponding parser frame has returned.
            if self.prev_ends_operand {
                if matches!(kind, Kind::Lpar | Kind::Lsqb | Kind::Dot) {
                    self.top().ast_loose += 1;
                    self.ast_total += 1;
                }
                if let Some(precedence) = binary_precedence(kind) {
                    self.observe_binary(precedence);
                }
            }
        }
        // AST operators are checked before the token releases prefix frames.
        // A later reset must not hide a transient violation from the old pass.
        let ast_exceeded = self.ast_exceeded();
        let exceeded = match kind {
            Kind::Lpar | Kind::Lsqb => self.push(LevelKind::Bracket),
            Kind::Lbrace => {
                let kind = match self.top().kind {
                    LevelKind::FString | LevelKind::Interpolation { in_spec: true } => {
                        LevelKind::Interpolation { in_spec: false }
                    }
                    LevelKind::Top
                    | LevelKind::Bracket
                    | LevelKind::Interpolation { in_spec: false } => LevelKind::Bracket,
                };
                self.push(kind)
            }
            Kind::FStringStart => self.push(LevelKind::FString),
            Kind::Rpar | Kind::Rsqb | Kind::Rbrace | Kind::FStringEnd => {
                self.pop();
                false
            }
            Kind::Indent => {
                self.indent += 1;
                self.elif_runs.push(0);
                self.match_blocks.push(self.pending_match);
                self.pending_match = false;
                self.add(1)
            }
            Kind::Dedent => {
                if self.indent > 0 {
                    self.indent -= 1;
                    self.total -= 1;
                    self.ast_total -= 1 + self.elif_runs.pop().unwrap_or(0);
                    self.match_blocks.pop();
                }
                false
            }
            Kind::Minus | Kind::Plus if self.in_pattern => self.add_tight(1),
            // Binary after an operand (`a - b`, `a * b`, `a not in b`), prefix otherwise.
            Kind::Minus | Kind::Plus | Kind::Star | Kind::Not => {
                if self.prev_ends_operand {
                    self.reset_tight();
                    false
                } else {
                    self.add_tight(1)
                }
            }
            // `**` as a binary operator recurses for its right operand; as
            // unpacking it is merely overcounted.
            Kind::Tilde | Kind::Await | Kind::Yield | Kind::Async | Kind::DoubleStar => {
                self.add_tight(1)
            }
            Kind::Lambda => {
                self.top().pending_lambdas += 1;
                false
            }
            Kind::Match if starts_statement => {
                self.pending_match = true;
                false
            }
            Kind::Case if starts_statement && self.match_blocks.last() == Some(&true) => {
                self.in_pattern = true;
                false
            }
            Kind::Colon => self.observe_colon(),
            // The `orelse` branch recurses once itself and once for its expression.
            Kind::Else => self.add_loose(2),
            // A comma between `lambda` and its `:` separates parameters.
            Kind::Comma if self.top().pending_lambdas > 0 => false,
            Kind::Comma => {
                self.reset_level();
                false
            }
            Kind::Equal => {
                self.in_pattern = false;
                self.reset_level();
                false
            }
            Kind::Newline | Kind::Semi => {
                self.reset_level();
                false
            }
            // Lower precedence than every prefix operator and `**`, so those frames have returned.
            Kind::Slash
            | Kind::DoubleSlash
            | Kind::Percent
            | Kind::At
            | Kind::Vbar
            | Kind::Amper
            | Kind::CircumFlex
            | Kind::LeftShift
            | Kind::RightShift
            | Kind::Less
            | Kind::Greater
            | Kind::EqEqual
            | Kind::NotEqual
            | Kind::LessEqual
            | Kind::GreaterEqual
            | Kind::In
            | Kind::Is
            | Kind::And
            | Kind::Or
            | Kind::If
            | Kind::For => {
                self.reset_tight();
                false
            }
            _ => false,
        };
        self.prev_ends_operand = matches!(
            kind,
            Kind::Name
                | Kind::Case
                | Kind::Int
                | Kind::String
                | Kind::FStringEnd
                | Kind::Rpar
                | Kind::Rsqb
                | Kind::Rbrace
        );
        self.statement_start = matches!(
            kind,
            Kind::Newline | Kind::Semi | Kind::Indent | Kind::Dedent
        );
        if matches!(kind, Kind::Newline | Kind::Semi) {
            self.in_pattern = false;
        }
        if exceeded {
            Some(Reason::Nesting)
        } else if ast_exceeded || self.ast_exceeded() {
            Some(Reason::AstDepth)
        } else {
            None
        }
    }

    fn observe_statement_start(&mut self, kind: Kind) {
        if !self.statement_start {
            return;
        }
        if kind == Kind::Elif {
            *self.elif_runs.last_mut().unwrap() += 1;
            self.ast_total += 1;
            return;
        }
        if matches!(
            kind,
            Kind::Newline | Kind::Semi | Kind::Indent | Kind::Dedent | Kind::Else
        ) {
            return;
        }
        let run = self.elif_runs.last_mut().unwrap();
        self.ast_total -= *run;
        *run = 0;
    }

    /// A lambda's `:` opens its body; any other colon ends the expression
    /// before it, and inside a replacement field opens the format spec.
    fn observe_colon(&mut self) -> bool {
        let top = self.top();
        if top.pending_lambdas > 0 {
            top.pending_lambdas -= 1;
            // The body recurses once itself and once for its expression.
            self.add_loose(2)
        } else {
            self.reset_level();
            let top = self.top();
            if let LevelKind::Interpolation { in_spec } = &mut top.kind {
                *in_spec = true;
                self.add_loose(1)
            } else {
                false
            }
        }
    }

    fn top(&mut self) -> &mut Level {
        self.levels
            .last_mut()
            .expect("the bottom level is never popped")
    }

    fn push(&mut self, kind: LevelKind) -> bool {
        self.levels.push(Level::new(kind));
        self.add(1)
    }

    fn pop(&mut self) {
        if self.levels.len() > 1 {
            let level = self.levels.pop().expect("length checked above");
            let shared = 1 + level.tight + level.loose;
            self.total -= shared;
            self.ast_total -= shared + level.ast_loose + level.binary_depth;
        }
    }

    fn add_tight(&mut self, frames: u32) -> bool {
        self.top().tight += frames;
        self.add(frames)
    }

    fn add_loose(&mut self, frames: u32) -> bool {
        self.top().loose += frames;
        self.add(frames)
    }

    fn reset_tight(&mut self) {
        let top = self.top();
        let released = top.tight;
        top.tight = 0;
        self.total -= released;
        self.ast_total -= released;
    }

    fn reset_level(&mut self) {
        let top = self.top();
        let released = top.tight + top.loose;
        let ast_released = released + top.ast_loose + top.binary_depth;
        top.tight = 0;
        top.loose = 0;
        top.binary_depth = 0;
        top.binary_len = 0;
        top.ast_loose = 0;
        self.total -= released;
        self.ast_total -= ast_released;
    }

    fn observe_binary(&mut self, precedence: u8) {
        let level = self.top();
        let mut current = 0;
        while level.binary_stack[..level.binary_len]
            .last()
            .is_some_and(|(p, _)| *p >= precedence)
        {
            level.binary_len -= 1;
            let (_, left) = level.binary_stack[level.binary_len];
            current = left.max(current) + 1;
        }
        level.binary_stack[level.binary_len] = (precedence, current);
        level.binary_len += 1;
        let depth = level.binary_stack[..level.binary_len]
            .iter()
            .rev()
            .fold(0u32, |right, (_, left)| left.max(&right) + 1);
        let old_depth = level.binary_depth;
        level.binary_depth = depth;
        self.ast_total = self.ast_total - old_depth + depth;
    }

    fn ast_exceeded(&self) -> bool {
        self.ast_limit.is_some_and(|limit| self.ast_total > limit)
    }

    fn add(&mut self, frames: u32) -> bool {
        self.total += frames;
        self.ast_total += frames;
        self.total > self.limit
    }
}

impl Level {
    fn new(kind: LevelKind) -> Self {
        Self {
            kind,
            tight: 0,
            loose: 0,
            pending_lambdas: 0,
            binary_stack: [(0, 0); 6],
            binary_len: 0,
            binary_depth: 0,
            ast_loose: 0,
        }
    }
}

fn binary_precedence(kind: Kind) -> Option<u8> {
    match kind {
        Kind::Vbar => Some(1),
        Kind::CircumFlex => Some(2),
        Kind::Amper => Some(3),
        Kind::LeftShift | Kind::RightShift => Some(4),
        Kind::Plus | Kind::Minus => Some(5),
        Kind::Star | Kind::At | Kind::Slash | Kind::DoubleSlash | Kind::Percent => Some(6),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../tests/support/reference_rules.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{Lexer, LexerScratch};

    #[test]
    fn combined_pass_matches_independent_passes() {
        let alphabet = [
            "x", "1", "(", ")", "[", "]", "{", "}", "f'", "'", ":", ",", ";", "\n", " ", "+", "-",
            "*", "**", "~", "lambda", "else", "elif", "if", "await", ".", "|", "^", "&", "<<", "/",
            "=", "not", "for",
        ];
        let mut random = 42u64;

        for parser_limit in [0, 1, 3, 10, 30] {
            for ast_limit in [None, Some(0), Some(1), Some(5), Some(30)] {
                let mut combined = Scanner::new(parser_limit, ast_limit);
                let mut parser = reference::Scanner::<false>::new(parser_limit);
                let mut ast = ast_limit.map(reference::Scanner::<true>::new);
                let mut scratch = LexerScratch::new(30, 30);

                for _ in 0..500 {
                    let mut source = String::new();
                    for _ in 0..100 {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        source.push_str(alphabet[(random >> 32) as usize % alphabet.len()]);
                    }
                    combined.reset(parser_limit, ast_limit);
                    parser.reset(parser_limit);
                    if let Some(ast) = &mut ast {
                        ast.reset(ast_limit.unwrap());
                    }

                    for token in Lexer::new(&source, 30, 30, &mut scratch) {
                        let expected = if parser.observe(token.kind) {
                            Some(Reason::Nesting)
                        } else if ast.as_mut().is_some_and(|ast| ast.observe(token.kind)) {
                            Some(Reason::AstDepth)
                        } else {
                            None
                        };
                        assert_eq!(
                            combined.observe(token.kind),
                            expected,
                            "{source:?}: {:?}",
                            token.kind
                        );
                        if expected.is_some() {
                            break;
                        }
                        assert_eq!(combined.total, parser.total);
                        if let Some(ast) = &ast {
                            assert_eq!(combined.ast_total, ast.total);
                        }
                    }
                }
            }
        }
    }
}
