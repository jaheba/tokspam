# Repository role

Spam is an experimental Rust library that screens Python source before it reaches a parser.
It detects source shapes that may consume excessive parser or AST stack space, including deep nesting, indentation,
operator chains, and quoted annotations.

Spam is a resource-complexity guard, not a Python parser or a security policy for program behavior.
It does not execute input, build a Python AST, validate syntax, or restrict imports, names, calls, or runtime
capabilities.
Acceptance is not yet a verified guarantee that a downstream parser can safely process the source.

The intended integration is a preflight check at every parser entry point, including dynamically generated source passed
to `eval()` or `exec()`.
Spam is integrated at Monty's pre-parser source guard, while the library remains independent of Monty and Ruff.

# Implementation and safety

- `src/lib.rs` exposes `check`, reusable `Checker`, `Limits`, and rejection reasons, and coordinates annotation
  screening.
- `src/lexer.rs` provides a streaming lexer that retains source ranges without decoding literals or collecting a token
  list.
- `src/rules.rs` estimates parser recursion and AST depth from token structure.
- `tests/` covers configurable limits and allocation behavior.
- `fuzz/`, `tools/`, and `corpus/` support generated-input campaigns, parser comparisons, benchmarks, and retained
  regression cases.

The library has no runtime dependencies.
Scratch allocation count and sizes depend only on `Limits`; input must not trigger additional heap allocations during
scanning.
Oversized source is rejected before scratch allocation.

Read `SAFETY.md` before changing resource accounting or claiming parser safety.
It records the intended invariants and outstanding release requirements.
Quoted-annotation scanning uses a preallocated explicit work stack.
Hard configuration ceilings and shared byte and token budgets bound nested scans.
Preserve the distinction between these implemented bounds and parser-safety claims.

Use `README.md` for usage and tool commands, and `EXPLORATION.md` for experimental results and their limitations.
Parser campaign outcomes apply to the tested versions and conditions; they are not general proofs of safety.

# Development conventions

- Never include "codex" in Git branch names.
  Use short, descriptive names without an agent prefix.
- Use blank lines to separate logical steps and make code easy to scan.
- Add concise comments explaining non-obvious behavior, supported operand types, and why a fallback or special case is
  needed.
  Avoid comments that merely repeat the code.
- Keep source ranges on UTF-8 boundaries and preserve bounded behavior for malformed input as well as valid Python.
- For changes to scanning or resource bounds, run `cargo test` and `cargo clippy --all-targets -- -D warnings`.
  Use `cargo fmt` for formatting.
  Add focused regressions when changing screening or allocation behavior.
