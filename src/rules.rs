//! Recursion rules adapted from Monty's existing source-nesting guard.
use super::Reason;
use super::lexer::Kind;
use crate::stack::{Mark, STRUCTURAL_CAPACITY, Stack};

/// Estimates parser frames from lexical structure.
/// Brackets, operators, bodies, format specs and blocks consume depth.
/// Malformed-input coverage is not a verified downstream safety guarantee.
/// Parser and AST estimates share structural state and keep separate totals.
pub(super) struct Scanner {
    /// Open bracket levels, bottom entry being the module or expression itself.
    levels: Stack<Level, STRUCTURAL_CAPACITY>,
    /// True for indentation levels whose suite contains `case` clauses.
    match_blocks: Stack<bool, STRUCTURAL_CAPACITY>,
    /// Consecutive `elif` clauses at each indentation level. Python stores
    /// these as nested `If` nodes despite their flat source indentation.
    elif_runs: Stack<u32, STRUCTURAL_CAPACITY>,
    state: ScannerState,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ScannerState {
    limit: u32,
    ast_limit: Option<u32>,
    pub(super) ast_total: u32,
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
    /// Open levels (excluding the bottom), indentation and every level's counts.
    pub(super) total: u32,
}

const _: () = assert!(crate::MAX_AST_DEPTH as u32 + 6 <= u16::MAX as u32);

/// Frames open at one bracket level.
struct Level {
    kind: LevelKind,
    postfix: bool,
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
    // Separate arrays avoid tuple padding at every structural level. Depths
    // are AST-only: the hard AST ceiling plus six folds fits in u16.
    binary_precedences: [u8; 6],
    binary_left_depths: [u16; 6],
    binary_len: u8,
    expression_depth: u32,
    // Postfix/grouped height of the current operand, independent of siblings.
    operand_depth: u32,
    // Retain the deepest item when commas or closing brackets end a child scan.
    ast_peak: u32,
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
    #[cfg(test)]
    pub(super) fn new(limit: u32, ast_limit: Option<u32>) -> Self {
        Self::with_capacity(limit, ast_limit, limit as usize + 2)
    }

    pub(super) fn with_capacity(limit: u32, ast_limit: Option<u32>, capacity: usize) -> Self {
        let mut scanner = Self {
            levels: Stack::with_capacity(capacity),
            match_blocks: Stack::with_capacity(capacity),
            elif_runs: Stack::with_capacity(capacity),
            state: ScannerState::default(),
        };
        scanner.reset(limit, ast_limit);
        scanner
    }

    pub(super) fn suspend(&mut self) -> (ScannerState, [Mark; 3]) {
        (
            self.state,
            [
                self.levels.suspend(),
                self.match_blocks.suspend(),
                self.elif_runs.suspend(),
            ],
        )
    }

    pub(super) fn restore(&mut self, saved: (ScannerState, [Mark; 3])) {
        self.state = saved.0;
        self.levels.restore(saved.1[0]);
        self.match_blocks.restore(saved.1[1]);
        self.elif_runs.restore(saved.1[2]);
    }

    pub(super) fn clear_segments(&mut self) {
        self.levels.reset();
        self.match_blocks.reset();
        self.elif_runs.reset();
    }

    pub(super) fn reset(&mut self, limit: u32, ast_limit: Option<u32>) {
        self.state.limit = limit;
        self.state.ast_limit = ast_limit;
        self.state.ast_total = 0;
        self.levels.clear();
        self.levels.push(Level::new(LevelKind::Top));
        self.elif_runs.clear();
        self.elif_runs.push(0);
        self.state.indent = 0;
        self.state.prev_ends_operand = false;
        self.state.statement_start = true;
        self.state.in_pattern = false;
        self.state.pending_match = false;
        self.match_blocks.clear();
        self.match_blocks.push(false);
        self.state.total = 0;
    }

    #[cfg(feature = "regions")]
    pub(super) fn region_budget(&self) -> Option<usize> {
        // Called only after a logical newline/layout token. These boundaries
        // release local frames; only unfinished lambda parameters can survive.
        debug_assert!(self.state.statement_start && self.levels.len() == 1);
        let top = &self.levels[0];
        debug_assert!(
            top.tight == 0 && top.loose == 0 && top.operand_depth == 0 && top.expression_depth == 0
        );
        if top.pending_lambdas != 0 {
            return None;
        }
        let remaining = self.state.limit.saturating_sub(self.state.total);
        Some(
            remaining.min(self.state.ast_limit.map_or(remaining, |limit| {
                limit.saturating_sub(self.state.ast_total)
            })) as usize,
        )
    }

    #[cfg(feature = "regions")]
    pub(super) fn finish_region(&mut self, seen_code: bool) {
        // First-token elif/else and pending lambdas are excluded by the proof.
        // A normal first token clears this indent's branch run, then newline
        // leaves all local counters empty while ancestor runs remain retained.
        if seen_code && self.state.ast_limit.is_some() {
            self.observe_statement_start(Kind::Name);
        }
        self.state.prev_ends_operand = false;
        self.state.statement_start = true;
    }

    /// Updates both estimates, preserving parser rejection priority on a token.
    #[inline]
    pub(super) fn observe(&mut self, kind: Kind) -> Option<Reason> {
        let starts_statement = self.state.statement_start;
        if self.state.ast_limit.is_some() {
            self.observe_statement_start(kind);

            // Calls, subscripts and attributes retain their left operand in the
            // AST after the corresponding parser frame has returned.
            if self.state.prev_ends_operand {
                if matches!(kind, Kind::Lpar | Kind::Lsqb | Kind::Dot) {
                    self.top().operand_depth += 1;
                    self.refresh_expression_depth();
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
            Kind::Lpar | Kind::Lsqb => self.push(LevelKind::Bracket, self.state.prev_ends_operand),
            Kind::Lbrace => {
                let kind = match self.top().kind {
                    LevelKind::FString | LevelKind::Interpolation { in_spec: true } => {
                        LevelKind::Interpolation { in_spec: false }
                    }
                    LevelKind::Top
                    | LevelKind::Bracket
                    | LevelKind::Interpolation { in_spec: false } => LevelKind::Bracket,
                };
                self.push(kind, false)
            }
            Kind::FStringStart => self.push(LevelKind::FString, false),
            Kind::Rpar | Kind::Rsqb | Kind::Rbrace | Kind::FStringEnd => {
                self.pop();
                false
            }
            Kind::Indent => {
                self.state.indent += 1;
                self.elif_runs.push(0);
                self.match_blocks.push(self.state.pending_match);
                self.state.pending_match = false;
                self.add(1)
            }
            Kind::Dedent => {
                if self.state.indent > 0 {
                    self.state.indent -= 1;
                    self.state.total -= 1;
                    self.state.ast_total -= 1 + self.elif_runs.pop().unwrap_or(0);
                    self.match_blocks.pop();
                }
                false
            }
            Kind::Minus | Kind::Plus if self.state.in_pattern => self.add_tight(1),
            // Binary after an operand (`a - b`, `a * b`, `a not in b`), prefix otherwise.
            Kind::Minus | Kind::Plus | Kind::Star | Kind::Not => {
                if self.state.prev_ends_operand {
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
                self.state.pending_match = true;
                false
            }
            Kind::Case if starts_statement && self.match_blocks.last() == Some(&true) => {
                self.state.in_pattern = true;
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
                self.state.in_pattern = false;
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
        self.state.prev_ends_operand = matches!(
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
        self.state.statement_start = matches!(
            kind,
            Kind::Newline | Kind::Semi | Kind::Indent | Kind::Dedent
        );
        if matches!(kind, Kind::Newline | Kind::Semi) {
            self.state.in_pattern = false;
        }
        if self.state.ast_limit.is_some() {
            let top = self.top();
            top.ast_peak = top
                .ast_peak
                .max(top.tight + top.loose + top.expression_depth);
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
        if !self.state.statement_start {
            return;
        }
        if kind == Kind::Elif {
            *self.elif_runs.last_mut().unwrap() += 1;
            self.state.ast_total += 1;
            return;
        }
        if matches!(
            kind,
            Kind::Newline | Kind::Semi | Kind::Indent | Kind::Dedent | Kind::Else
        ) {
            return;
        }
        let run = self.elif_runs.last_mut().unwrap();
        self.state.ast_total -= *run;
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

    fn push(&mut self, kind: LevelKind, postfix: bool) -> bool {
        if self.state.ast_limit.is_some() {
            let parent = self.top();
            // A child's active path crosses pending operators, not the saved
            // left subtrees. Those heights were checked before suspension.
            let saved = parent.expression_depth;
            // Keep the conservative call/subscript wrapper charge while
            // descending into arguments; only completed callee height is suspended.
            let active = u32::from(parent.binary_len) + u32::from(postfix);
            self.state.ast_total = self.state.ast_total - saved + active;
        }
        let mut child = Level::new(kind);
        child.postfix = postfix;
        self.levels.push(child);
        self.add(1)
    }

    fn pop(&mut self) {
        if self.levels.len() > 1 {
            let level = self.levels.pop().expect("length checked above");
            let shared = 1 + level.tight + level.loose;
            self.state.total -= shared;
            self.state.ast_total -= shared + level.expression_depth;
            if self.state.ast_limit.is_some() {
                // Closed children still belong to the current operand. Preserve
                // their peak before a following postfix or binary operator wraps it.
                let parent = self.top();
                let saved = parent.expression_depth;
                let active = u32::from(parent.binary_len) + u32::from(level.postfix);
                parent.operand_depth = parent.operand_depth.max(1 + level.ast_peak);
                self.state.ast_total = self.state.ast_total - active + saved;
                self.refresh_expression_depth();
            }
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
        self.state.total -= released;
        self.state.ast_total -= released;
    }

    fn reset_level(&mut self) {
        let top = self.top();
        let released = top.tight + top.loose;
        let ast_released = released + top.expression_depth;
        top.tight = 0;
        top.loose = 0;
        top.expression_depth = 0;
        top.binary_len = 0;
        top.operand_depth = 0;
        self.state.total -= released;
        self.state.ast_total -= ast_released;
    }

    fn observe_binary(&mut self, precedence: u8) {
        let level = self.top();
        let mut current = level.operand_depth;
        level.operand_depth = 0;
        // Equal precedence folds left: a + b + c deepens the retained left
        // subtree. Higher precedence must finish before a looser operator.
        while level.binary_precedences[..usize::from(level.binary_len)]
            .last()
            .is_some_and(|p| *p >= precedence)
        {
            level.binary_len -= 1;
            let left = u32::from(level.binary_left_depths[usize::from(level.binary_len)]);
            current = left.max(current) + 1;
        }
        let index = usize::from(level.binary_len);
        level.binary_precedences[index] = precedence;
        level.binary_left_depths[index] = u16::try_from(current)
            .expect("binary depth is bounded by the hard AST ceiling plus six folds");
        level.binary_len += 1;
        self.refresh_expression_depth();
    }

    fn refresh_expression_depth(&mut self) {
        let level = self.top();
        // Combine sibling operand heights by maximum rather than adding their
        // postfix chains. The fixed precedence stack has at most six entries.
        let depth = level.binary_left_depths[..usize::from(level.binary_len)]
            .iter()
            .rev()
            .fold(level.operand_depth, |right, left| {
                u32::from(*left).max(right) + 1
            });
        let old_depth = level.expression_depth;
        level.expression_depth = depth;
        self.state.ast_total = self.state.ast_total - old_depth + depth;
    }

    fn ast_exceeded(&self) -> bool {
        self.state
            .ast_limit
            .is_some_and(|limit| self.state.ast_total > limit)
    }

    fn add(&mut self, frames: u32) -> bool {
        self.state.total += frames;
        self.state.ast_total += frames;
        self.state.total > self.state.limit
    }
}

impl Level {
    fn new(kind: LevelKind) -> Self {
        Self {
            kind,
            postfix: false,
            tight: 0,
            loose: 0,
            pending_lambdas: 0,
            binary_precedences: [0; 6],
            binary_left_depths: [0; 6],
            binary_len: 0,
            expression_depth: 0,
            operand_depth: 0,
            ast_peak: 0,
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

impl Default for Level {
    fn default() -> Self {
        Self::new(LevelKind::Top)
    }
}

impl std::ops::Deref for Scanner {
    type Target = ScannerState;
    fn deref(&self) -> &ScannerState {
        &self.state
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
    fn pending_lambda_counts_keep_the_full_token_budget_range() {
        let mut scanner = Scanner::new(1, None);
        for _ in 0..=u16::MAX {
            assert_eq!(scanner.observe(Kind::Lambda), None);
        }
        assert_eq!(scanner.state.total, 0);
        assert_eq!(scanner.top().pending_lambdas, u32::from(u16::MAX) + 1);
    }

    #[test]
    fn flat_arithmetic_still_matches_frozen_ast_reference() {
        for source in [
            "x = a + b * c - d / e | f ^ g & h << i",
            "x = a + b + c + d + e + f",
            "x = a * b + c * d + e * f",
        ] {
            for limit in [0, 1, 2, 5, 30] {
                let mut scanner = Scanner::new(100, Some(limit));
                let mut reference = reference::Scanner::<true>::new(limit);
                let mut scratch = LexerScratch::default();
                for token in Lexer::new(source, 100, 100, &mut scratch) {
                    let exceeded = reference.observe(token.kind);
                    let actual = scanner.observe(token.kind);
                    assert_eq!(actual == Some(Reason::AstDepth), exceeded);
                    assert_eq!(scanner.ast_total, reference.total);
                    if exceeded {
                        break;
                    }
                }
            }
        }
    }

    #[test]
    fn combined_pass_preserves_independent_parser_accounting() {
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
                let mut scratch = LexerScratch::default();

                for _ in 0..500 {
                    let mut source = String::new();
                    for _ in 0..100 {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        source.push_str(alphabet[(random >> 32) as usize % alphabet.len()]);
                    }
                    combined.reset(parser_limit, ast_limit);
                    parser.reset(parser_limit);

                    for token in Lexer::new(&source, 30, 30, &mut scratch) {
                        let parser_exceeded = parser.observe(token.kind);
                        let actual = combined.observe(token.kind);
                        if parser_exceeded {
                            assert_eq!(actual, Some(Reason::Nesting), "{source:?}");
                        } else if ast_limit.is_none() {
                            assert_eq!(actual, None, "{source:?}");
                        } else {
                            assert_ne!(actual, Some(Reason::Nesting), "{source:?}");
                        }
                        assert_eq!(combined.total, parser.total);
                        if actual.is_some() {
                            break;
                        }
                    }
                }
            }
        }
    }
}
