# Repository role

Tokspam is a Rust library that screens Python source before it reaches a parser.
It detects source shapes that may consume excessive parser or AST stack space, including deep nesting, indentation,
operator chains, and quoted annotations.

Tokspam is a resource-complexity guard, not a Python parser or a security policy for program behavior.
It does not execute input, build a Python AST, validate syntax, or restrict imports, names, calls, or runtime
capabilities.
Acceptance is not yet a verified guarantee that a downstream parser can safely process the source.

The intended integration is a preflight check at every parser entry point, including dynamically generated source passed
to `eval()` or `exec()`.
Tokspam is integrated at Monty's pre-parser source guard, while the library remains independent of Monty and Ruff.

# Implementation and safety

- `src/lib.rs` exposes the public API; `src/limits.rs` defines and validates configuration.
- `src/checker/mod.rs` coordinates scanning and quoted annotations. Its child modules own shared scratch,
  annotation-position heuristics, and construct-width tracking.
- `src/lexer/mod.rs` coordinates the streaming lexer and shared state. Its child modules separate code dispatch,
  strings, indentation, byte searches, token types, and conservative proofs without collecting a token list.
- `src/rules.rs` estimates parser recursion and AST depth from token structure.
- `tests/` covers configurable limits and allocation behavior.
- Fuzzing, corpus generation, parser comparisons, and benchmarks live in the separate `tokspam-fuzzer` project.

The sole runtime dependency is `memchr`, used for optimized byte searches.
`Checker::new` uses empty vectors that grow on demand within limits-derived ceilings.
`Checker::preallocated` reserves buffers at construction; only this mode guarantees allocation-free scanning.
Both modes retain capacity between checks. Growth must never exceed configured scratch ceilings.
Oversized source is rejected before scratch allocation.

Read `docs/SAFETY.md` before changing resource accounting or claiming parser safety.
It records the intended invariants and outstanding release requirements.
Quoted-annotation scanning uses a bounded explicit work stack.
Hard configuration ceilings and shared byte and token budgets bound nested scans.
Preserve the distinction between these implemented bounds and parser-safety claims.

Use `README.md` and `docs/CONFIGURATION.md` for library usage.
Benchmark results and tooling documentation live in `tokspam-fuzzer`.
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

# Git approval

- Every commit and push requires explicit user confirmation for that action.
- Do not commit or push autonomously. Prepare the changes and ask for confirmation first, unless the user has already explicitly requested the commit or push.
