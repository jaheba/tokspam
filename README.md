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

`Limits` also controls token count, cumulative scan work, indentation depth, scanning quoted annotations, and the
maximum number of nested annotations. Scan work is the sum of bytes admitted to scans and tokens emitted, including
quoted annotations interpreted as Python. Exhausting that budget returns `Reason::Inconclusive` rather than claiming a
specific structural violation.
`check` returns a reason and a byte range for rejected input.
For repeated use, create a `Checker` once per worker.
Construction allocates scratch storage; every subsequent check performs zero heap allocations, including the first check
and checks after a rejection.

```rust
use spam::{Checker, Limits};

let mut checker = Checker::new(Limits::default()).expect("valid limits");
checker.check("x = [1, 2, 3]")?;
checker.check("y = x[0]")?;
# Ok::<(), spam::Rejection>(())
```

`Checker::new` validates configuration before allocating and returns `ConfigError` for a value above an implementation
ceiling.
`Limits::validate` performs the same check without constructing a checker.
The one-shot `check` function reports `Reason::Configuration` at `0..0` for invalid settings.

| Limit | Hard maximum |
| --- | ---: |
| Source bytes | 16 MiB |
| Tokens | 1,000,000 |
| Token bytes | 16 MiB |
| Delimiter/parser depth | 1,024 |
| Indentation depth | 1,024 |
| Nested quoted annotations | 32 |
| Estimated AST depth | 4,096 |
| Each experimental construct width | 1,000,000 |

The optional AST check counts long binary, attribute, call, and subscript chains that a parser may construct iteratively
but later destroy recursively.
Its depth estimate is conservative.

A shared structural pass maintains separate parser and AST depth estimates.
A conservative fast path scans a prefix of short, unindented simple lines without constructing tokens.
It counts tokens exactly and uses line byte length as a depth upper bound; complex syntax or insufficient budgets falls
back to detailed scanning.

The lexer handles comments, strings, f-strings, template strings, indentation, and explicit line continuations.
It retains bounded stacks and scans without storing a token list.
Scratch storage is allocated before token scanning, with allocation count and sizes determined only by `Limits`.
Input cannot trigger further heap allocations, including while scanning nested quoted annotations.
Quoted annotations are scanned with an explicit preallocated work stack; source structure never adds native Rust stack
frames.
Source exceeding the byte limit is rejected before allocating scratch storage.
String contents are screened only at likely annotation positions, including variable, parameter, and return annotations.
This is a lexical heuristic, not a Python grammar check.
Escaped and concatenated annotations are not decoded.

Spam exposes independent optional budgets for call arguments, function and lambda parameters, display items, subscript
items, and f-string or template-string interpolation fields.
All five default to `None`, so upgrading does not enable them automatically.
The former `experimental-width-limits` Cargo feature is a compatibility no-op.
The first calibrated profile uses 4,096 for calls, parameters, slices, and interpolation fields and 8,192 for displays.
These values are starting points rather than stable defaults; see the [width-limit
experiment](campaigns/2026-10-24-width-limits/SUMMARY.md).

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

Pass `--spam-check check-widths` to replay a campaign with the calibrated width profile.
`tools/differential_widths.py` compares exact Spam rejection boundaries with CPython AST widths across ordinary and
unusual valid constructs:

`tools/differential_generated.py` creates deterministic CPython-valid mixed programs, checks every width at the exact
AST boundary and one below, optionally invokes Pyrefly, minimizes mismatches, and records retained findings in SQLite.
The 2026-10-24 run checked 1,000 CPython/Ruff cases and 100 CPython/Ruff/Pyrefly cases without a width mismatch or
parser-process failure.

```sh
cargo build --release --all-features --example probe
python3 tools/differential_widths.py --spam-probe target/release/examples/probe
```

The generator explores grammar families and writes `results.jsonl`, `summary.json`, and source files for parser failures
and sources both parsers accept but Spam rejects.
Import the findings into the SQLite database with `python3 tools/case_db.py import /tmp/spam-parser-campaign`.
Cases receive `parser-failure` or `conservative-rejection` labels.
Import keeps the shortest source per family and observed outcome within each campaign, deduplicates by SHA-256, and
records results from every available implementation.
Run `python3 tools/case_db.py list` to inspect retained cases.

Resource campaigns use `resource-summary.json` and `resource-results.jsonl`.
Import them with:

```sh
python3 tools/case_db.py import-resource /path/to/campaign
```

Each retained resource case stores the exact source and an optional generator recipe.
Phase-specific observations record the outcome, parser status, raw timing samples, median, process memory, baseline
memory, and the time or memory threshold used to classify the result.
The `resource-failure` and `resource-control` labels distinguish threshold crossings from nearby controls;
`parser-resource`, `compiler-resource`, and `downstream-checker` identify the measured phase or subsystem.
Pyrefly checker findings use `downstream-checker` because they do not establish parser cost.

`tools/calibrate.py` measures valid local corpora before construct limits are chosen.
It records source and token sizes, AST and statement counts, identifier counts, maximum construct widths, Spam outcome,
CPython parse and compile samples, and optional Ruff wall time.
For example:

```sh
python3 tools/calibrate.py \
  --stdlib \
  --root /path/to/project --root-name project \
  --spam-probe target/release/examples/probe \
  --ruff-probe tools/ruff-probe/target/release/spam-ruff-probe \
  --database corpus/cases.sqlite \
  --output /tmp/spam-calibration.json
```

Calibration observations are stored separately from adversarial cases.
The checked-in CPython 3.14, Monty, and Pydantic results are summarized in
`campaigns/2026-10-24-calibration/SUMMARY.md`.

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
The database stores source bytes, generator origin, run settings, and per-implementation results. Every observation has
an explicit phase and, when available, an implementation version or probe digest. Opening an older database for writing
backfills this context from its run metadata.
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

Monty uses Spam at its existing pre-parser boundary, including source generated by `eval()` and `exec()`.
Monty configures only parser-recursion limits there; its converter retains the exact AST-depth checks and error
messages.

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

Check growth across flat statements, operator chains, f-strings, and quoted annotations with:

```sh
python3 tools/check_scaling.py --benchmark target/release/examples/benchmark --assert-linear
```

The script records a log-log runtime exponent for each family. It is a regression signal rather than a proof of a
worst-case bound; the process deadline remains the availability backstop.

`tools/benchmark.py --byte-guard` also measures an experimental acceptance-only byte guard plus fallback to the normal
checker.
This prototype is confined to benchmarks and tests; it is not enabled in the library because late fallback can increase
total screening time.
Its coverage, results, and limitations are recorded in the exploration log.

The `experimental-regions` feature exposes `Checker::check_regions`, which attempts bounded statement proofs and resumes
detailed scanning locally when a proof fails.
It shares existing scratch storage and performs no allocations per input.
The normal `Checker::check` path remains unchanged; enabling the feature does not select the experimental method
automatically.

```sh
cargo build --release --features experimental-regions --example benchmark
python3 tools/benchmark.py \
  --spam-bench target/release/examples/benchmark \
  --ruff-probe tools/ruff-probe/target/release/spam-ruff-probe \
  --regions --stdlib-file json/decoder.py --stdlib-file re/__init__.py \
  --output /tmp/spam-region-benchmark.json
```

Use `cargo test --features experimental-regions` to include the region differential and allocation checks.
The experiment reduces overhead on the tested general modules, but does not meet the 10% target there.

To attribute region-check costs without adding production instrumentation:

```sh
python3 tools/profile_regions.py --revision HEAD --output /tmp/spam-profile.json
```

The script builds an offline temporary snapshot and reports sampled proof, detailed-scanning, and annotation costs
alongside instrumentation overhead.
Use `--stdlib-file shutil.py` to select modules.
`--live-lexer` reproduces the historical lexer-lifetime experiment on revision `e236a65`; current revisions already
retain the lexer until an annotation suspends it.

To measure lexer changes independently of structural accounting, build
`cargo build --release --example lexer_benchmark --example benchmark` and save
both binaries before editing. Rebuild after the change, then compare:

```sh
python3 tools/profile_lexer.py \
  --baseline-lexer /tmp/lexer-before --baseline-checker /tmp/checker-before \
  --lexer target/release/examples/lexer_benchmark \
  --checker target/release/examples/benchmark --output /tmp/lexer-profile.json
python3 tools/check_lexer_equivalence.py \
  --baseline /tmp/lexer-before --lexer target/release/examples/lexer_benchmark \
  --output /tmp/lexer-equivalence.json
```

The profile compares isolated lexing and complete reusable checks, including
standard-library modules and synthetic hot paths. The equivalence tool compares
exact token streams on malformed inputs, standard-library source, and the retained
corpus. The lexer example also provides `--sample` for a ten-second CPU sampling
run on source piped through stdin.
