//! Recursion rules adapted from Monty's existing source-nesting guard.
use super::lexer::Kind;

/// Estimates parser frames from lexical structure.
/// Brackets, operators, bodies, format specs and blocks consume depth.
/// Coverage of malformed input remains experimental.
pub(super) struct Scanner {
    limit: u32,
    /// Open bracket levels, bottom entry being the module or expression itself.
    levels: Vec<Level>,
    /// Open indented blocks.
    indent: u32,
    /// Whether the previous token can end an operand, making a following
    /// `-`, `+`, `*` or `not` binary rather than prefix.
    prev_ends_operand: bool,
    /// Whether the next non-layout token starts a logical statement.
    statement_start: bool,
    /// Consecutive `elif` clauses at each indentation level. Python stores
    /// these as nested `If` nodes despite their flat source indentation.
    elif_runs: Vec<u32>,
    /// Open levels (excluding the bottom), indentation and every level's counts.
    pub(super) total: u32,
    ast: bool,
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
    pub(super) fn new(limit: u32, ast: bool) -> Self {
        // Include the root and one overflowing entry: observe reports the
        // rejection after pushing the token that exceeds the budget.
        let mut scanner = Self {
            limit,
            levels: Vec::with_capacity(limit as usize + 2),
            indent: 0,
            prev_ends_operand: false,
            statement_start: true,
            elif_runs: Vec::with_capacity(limit as usize + 2),
            total: 0,
            ast,
        };
        scanner.reset(limit);
        scanner
    }

    pub(super) fn reset(&mut self, limit: u32) {
        self.limit = limit;
        self.levels.clear();
        self.levels.push(Level::new(LevelKind::Top));
        self.elif_runs.clear();
        self.elif_runs.push(0);
        self.indent = 0;
        self.prev_ends_operand = false;
        self.statement_start = true;
        self.total = 0;
    }

    /// Accounts for one token, returning whether the estimate now exceeds the limit.
    pub(super) fn observe(&mut self, kind: Kind) -> bool {
        if self.observe_statement_start(kind) {
            return true;
        }
        // A call or subscript keeps its left operand in the resulting AST after
        // the closing delimiter releases the parser frame.
        if self.ast
            && self.prev_ends_operand
            && matches!(kind, Kind::Lpar | Kind::Lsqb)
            && self.add_loose(1)
        {
            return true;
        }
        if self.ast && self.prev_ends_operand {
            if kind == Kind::Dot && self.add_loose(1) {
                return true;
            }
            if let Some(precedence) = binary_precedence(kind) {
                if self.observe_binary(precedence) {
                    return true;
                }
            }
        }
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
                self.add(1)
            }
            Kind::Dedent => {
                if self.indent > 0 {
                    self.indent -= 1;
                    self.total -= 1 + self.elif_runs.pop().unwrap_or(0);
                }
                false
            }
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
            Kind::Colon => self.observe_colon(),
            // The `orelse` branch recurses once itself and once for its expression.
            Kind::Else => self.add_loose(2),
            // A comma between `lambda` and its `:` separates parameters.
            Kind::Comma if self.top().pending_lambdas > 0 => false,
            Kind::Comma => {
                self.reset_level();
                false
            }
            Kind::Newline | Kind::Semi | Kind::Equal => {
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
        exceeded
    }

    fn observe_statement_start(&mut self, kind: Kind) -> bool {
        if !self.ast || !self.statement_start {
            return false;
        }
        if kind == Kind::Elif {
            *self.elif_runs.last_mut().unwrap() += 1;
            return self.add(1);
        }
        if matches!(
            kind,
            Kind::Newline | Kind::Semi | Kind::Indent | Kind::Dedent | Kind::Else
        ) {
            return false;
        }
        let run = self.elif_runs.last_mut().unwrap();
        self.total -= *run;
        *run = 0;
        false
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
            self.total -= 1 + level.tight + level.loose + level.binary_depth;
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
    }

    fn reset_level(&mut self) {
        let top = self.top();
        let released = top.tight + top.loose + top.binary_depth;
        top.tight = 0;
        top.loose = 0;
        top.binary_depth = 0;
        top.binary_len = 0;
        self.total -= released;
    }

    fn observe_binary(&mut self, precedence: u8) -> bool {
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
        if depth >= old_depth {
            self.add(depth - old_depth)
        } else {
            self.total -= old_depth - depth;
            false
        }
    }

    fn add(&mut self, frames: u32) -> bool {
        self.total += frames;
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
