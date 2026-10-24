# Spam

Spam screens Python source for shapes that can consume excessive parser or AST stack space.
It has no runtime dependencies, constructs no Python AST, and does not execute input.
It does not inspect imports, names, calls, or runtime behavior.
It is experimental: passing a check is not a guarantee that a parser can safely process the source.
See [Spam safety requirements](SAFETY.md) for the no-recursion invariant, resource bounds, current gaps, and release
criteria.

```rust
use spam::{check, Limits};

let limits = Limits {
    max_source_bytes: 256 * 1024,
    max_nesting_depth: 80,
    max_ast_depth: Some(80),
    ..Limits::default()
};
check("x = [1, 2, 3]", &limits)?;
# Ok::<(), spam::Rejection>(())
```

`Limits` also controls token count, indentation depth, scanning quoted annotations, and the maximum number of nested
annotations.
`check` returns a reason and a byte range for rejected input.
For repeated use, create a `Checker` once per worker.
Construction allocates scratch storage; every subsequent check performs zero heap allocations, including the first check
and checks after a rejection.

```rust
use spam::{Checker, Limits};

let mut checker = Checker::new(Limits::default());
checker.check("x = [1, 2, 3]")?;
checker.check("y = x[0]")?;
# Ok::<(), spam::Rejection>(())
```

The optional AST check counts long binary, attribute, call, and subscript chains that a parser may construct iteratively
but later destroy recursively.
Its depth estimate is conservative.

A shared structural pass maintains separate parser and AST depth estimates. A conservative fast path scans a prefix
of short, unindented simple lines without constructing tokens. It counts tokens exactly and uses line byte length as
a depth upper bound; complex syntax or insufficient budgets falls back to detailed scanning.

The lexer handles comments, strings, f-strings, template strings, indentation, and explicit line continuations.
It retains bounded stacks and scans without storing a token list.
Scratch storage is allocated before token scanning, with allocation count and sizes determined only by `Limits`.
Input cannot trigger further heap allocations, including while scanning nested quoted annotations.
Source exceeding the byte limit is rejected before allocating scratch storage.
String contents are screened only at likely annotation positions, including variable, parameter, and return annotations.
This is a lexical heuristic, not a Python grammar check.
Escaped and concatenated annotations are not decoded.

## Tests and fuzzing

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

`fuzz/` contains libFuzzer targets for arbitrary UTF-8 input and grammar-generated Python.
The grammar generator is shared with `tools/fuzz_parsers.py` and includes the binary, attribute, call, and subscript
chains from [Monty issue #1042](https://github.com/pydantic/monty/issues/1042), plus nested targets from [issue
#112](https://github.com/pydantic/monty/issues/112).
The parser campaign runs CPython and Ruff in disposable subprocesses with a timeout and a source-size cap.
It records crashes, timeouts, `MemoryError`, and `RecursionError` separately from syntax-limit rejections.
Linux also gives each child an address-space limit; other platforms rely on source size and time limits.

Build the probes, then run a bounded campaign:

```sh
cargo build --example probe
cargo build --manifest-path tools/ruff-probe/Cargo.toml
cargo build --manifest-path tools/monty-probe/Cargo.toml
python3 tools/fuzz_parsers.py \
  --spam-probe target/debug/examples/probe \
  --ruff-probe tools/ruff-probe/target/debug/spam-ruff-probe \
  --monty-probe tools/monty-probe/target/debug/spam-monty-probe \
  --python python3 \
  --output /tmp/spam-parser-campaign
```

The generator explores grammar families and writes `results.jsonl`, `summary.json`, and source files for parser failures
and sources both parsers accept but Spam rejects.
Import the findings into the SQLite database with `python3 tools/case_db.py import /tmp/spam-parser-campaign`.
Cases receive `parser-failure` or `conservative-rejection` labels.
Import keeps the shortest source per family and observed outcome within each campaign, deduplicates by SHA-256, and
records results from every available implementation.
Run `python3 tools/case_db.py list` to inspect retained cases.

See the [parser attack plan](ATTACK_PLAN.md) for campaign budgets, attack families, and success criteria.
The [exploration log](EXPLORATION.md) records tested shapes, negative results, run conditions, and next experiments.
Run `python3 tools/scan_stdlib.py --spam-probe target/debug/examples/probe --ruff-probe
tools/ruff-probe/target/debug/spam-ruff-probe --output /tmp/spam-stdlib` under the Python version you want to sample.
It reports valid standard-library files that Spam rejects and checks those files with Ruff.

Generate focused valid and malformed cases with `tools/make_operator_cases.py`, `tools/make_nested_class_cases.py`,
`tools/make_invalid_cases.py`, and `tools/make_attack_cases.py`.
Each writes a manifest accepted by `tools/fuzz_parsers.py --manifest`.
The operator sweep covers arithmetic, matrix multiplication, shifts, bitwise and Boolean operators, comparisons,
membership and identity tests, unary operators, augmented assignments, and assignment expressions.
It tests Spam's screening behavior; Spam does not parse operator semantics.
`tools/minimize_case.py` performs isolated delta debugging on a retained CPython parser failure.
The minimized mixed-input failure is checked in under `corpus/minimized/` with a replay manifest.

Replay retained cases by adding `--database corpus/cases.sqlite` to the campaign command above, then import that
replay's output to record new observations.
The database stores source bytes, generator origin, run settings, and per-implementation results.
An observation is specific to the parser version, platform, phase, timeout, and memory limit of its run; a later run may
differ.
Use `--manifest examples/manifest.json` for four ordinary examples covering f-strings, quoted annotations, and
indentation.
The Ruff probe is pinned to `ruff_python_parser` 0.0.14, matching the Monty checkout used for the first campaign.
Ruff is a dependency of that probe only; it is absent from Spam's dependency graph.
The optional Monty probe uses a sibling `~/p/monty` checkout.
It compiles source by default.
Pass `--monty-mode run` to execute each program in an isolated process with Monty's time and memory limits.
Add `--issue-repro` to that mode to test [issue #1042](https://github.com/pydantic/monty/issues/1042)'s short `eval()`
program, which constructs a long binary chain at runtime.
The campaign records only crashes, timeouts, and parser resource failures as failures; normal Monty runtime exceptions
are data in the report.

In the first seed-42 campaign of 262 generated programs, CPython raised `MemoryError` on three deep expressions.
A later sweep of long shallow chains found `RecursionError` in CPython and stack-overflow aborts in Ruff 0.0.14 for
multiplication, XOR, attributes, calls, subscripts, and slices.
Ruff parsed 50,000-element chains in these families but aborted at 70,000 or 100,000.
Spam rejected each recorded failure.
These are bounded samples, not evidence that every harmful source shape is covered.
The issue #1042 `eval()` reproducer is a separate runtime case: Spam accepts its short outer source, and an isolated
Monty worker exits when the generated expression exceeds the configured allocator limit.
Screening dynamically generated source at each parser entry point remains essential.

Spam is not integrated into Monty yet.
Monty's existing source guard remains in place, including for source generated by `eval()` and `exec()`.
Before using Spam as a replacement, test parser recovery paths and false rejections and verify that every parser entry
point invokes it.

## Performance

Build the release benchmarks and run the comparison under the Python interpreter of interest:

```sh
cargo build --release --example benchmark
cargo build --release --manifest-path tools/ruff-probe/Cargo.toml
python3 tools/benchmark.py \
  --spam-bench target/release/examples/benchmark \
  --ruff-probe tools/ruff-probe/target/release/spam-ruff-probe \
  --output /tmp/spam-benchmark.json
```

The benchmark reports seven-sample medians for in-process Spam checking, Ruff parsing plus AST destruction, and CPython
`ast.parse`.
It excludes process startup.
The report includes both `check` (scratch construction and destruction on every call) and a reused `Checker`
(construction excluded, state reset included).
Spam screens rather than builds an AST, so the times show added preflight cost, not equivalent parsing work.
Pass `--baseline /path/to/previous/benchmark` to compare against an earlier binary supporting `--reuse`.
Use repeated `--stdlib-file` options, such as `--stdlib-file dataclasses.py`, to add representative installed modules.
The report includes screening time as a fraction of Ruff parsing time; early rejections should be assessed separately
from accepted-input overhead.
The measured results and machine details are in [the exploration log](EXPLORATION.md#performance).
