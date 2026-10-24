# Parser exploration log

This log records generated source shapes, including runs that found no parser failure.
`corpus/cases.sqlite` stores retained parser failures, conservative Spam rejections, and per-run observations.
The generators are in `tools/generator.rs`; `tools/fuzz_parsers.py` runs each parser in a disposable process.

## Conditions

The runs below used macOS arm64, CPython 3.14.2, `ruff_python_parser` 0.0.14, and Spam with a depth limit of 100.
The process stack limit was 8,176 KiB.
Each source was capped at 1 MiB.
Parser deadlines were two seconds for the broad campaigns and five seconds for the large-chain runs.
The macOS runner disabled core dumps but did not impose an address-space limit.
`ast.parse` was used for CPython; the Ruff probe parsed a module and dropped its AST before reporting success.
Neither probe executed the generated program.
An `ok` result therefore means this source parsed and its AST was dropped within the deadline under these conditions.

The campaign treats `MemoryError`, `RecursionError`, a signal, a nonzero process exit, and a timeout as failures.
CPython's `syntax-limit` and `syntax-error` results are recorded separately.
A syntax-limit is a controlled rejection, not a crash.
Ruff's `crash` below was signal 6, with `thread 'main' has overflowed its stack` on stderr.
The probe does not yet identify whether overflow occurs during parsing or AST destruction.

## Runs and results

| Run | Inputs | Parser failures | Spam accepted a failure | Finding |
| --- | ---: | ---: | ---: | --- |
| First grammar campaign, seed 42 | 262 | 3 | 0 | CPython `MemoryError` on deep power and lambda expressions; Ruff parsed them. |
| Expanded grammar campaign, seed 42 | 552 | 8 | 0 | More power and lambda `MemoryError` cases; no new failing family through depth 5,000. |
| Long shallow chains | 10 | 6 | 0 | CPython `RecursionError`; Ruff stack overflow on five chain families at depth 100,000. |
| Chain thresholds | 15 | 10 | 0 | For those five families, depth 30,000 parsed in both; CPython failed at 50,000; Ruff failed at 70,000. |
| Large unusual forms | 15 | 2 | 0 | Slices followed the same chain pattern, with Ruff failing at depth 100,000. |
| Mixed forms through depth 300 | 515 | 0 | 0 | Both parsers accepted 201 sources that Spam rejected at the configured limit. |
| Large mixed chains | 12 | 12 | 0 | Mixed postfixes failed Ruff at 50,000; f-string, comprehension, and operator chains failed at 70,000. |
| Nested classes and patterns, seed 42 | 451 | 0 | 0 | Nested class, class-pattern, and mapping-pattern shapes through depth 300 caused no parser failure. |
| Expression operator sweep | 129 | 0 | 0 | All main operator families at lengths 80, 256, and 1,024; two controlled CPython syntax limits. |
| Mutated invalid source | 330 | 11 | 0 | One mixed-expression mutation and ten deep power/lambda mutations raised CPython `MemoryError`. Ruff returned syntax errors. |
| Long additions in nested class bodies | 9 | 6 | 0 | CPython failed at 50,000 additions and Ruff aborted at 70,000, with 0, 20, or 80 enclosing classes. |
| Focused attack families | 30 | 2 | 2 before fix, 0 after | A flat `elif` chain exposed implicit CPython AST nesting. |
| Large flat and malformed inputs | 23 | 2 | 2 before fix, 0 after | Flat f-string fields timed out CPython; repeated invalid `if` tokens aborted Ruff. |

The first campaign included optional Monty compilation.
Monty returned a compile-time `SyntaxError` for the three CPython `MemoryError` cases.
The later high-depth runs did not include Monty.
The final replay after the attack-focused fixes ran 83 cases against CPython 3.14.2, Ruff, and Spam.
All 50 parser-failure cases reproduced a failure and Spam rejected each of them.
Twenty-five of the other cases still parsed in both implementations and were rejected by Spam.
The database has 50 parser-failure labels and 33 historical conservative-rejection labels; some labeled cases now pass
Spam after the false-positive fixes.
The database contains several sizes of the same failure shape to show thresholds.

### Shapes that failed

| Source shape | Smallest observed failure here | Other observation |
| --- | --- | --- |
| Right-associated `2 ** 2 ** ...` | CPython `MemoryError` near depth 3,000 | Ruff parsed the retained case. |
| Nested `lambda: lambda: ...` | CPython `MemoryError` near depth 3,000 | Ruff parsed the retained case. |
| Left-associated `1 * 1 * ...`, `1 ^ 1 ^ ...` | CPython `RecursionError` at 50,000 | Ruff parsed 50,000 and aborted at 70,000. |
| Repeated `.x`, `()`, `[0]` postfixes | CPython `RecursionError` at 50,000 | Ruff parsed 50,000 and aborted at 70,000. |
| Repeated `[0:1]` slices | CPython `RecursionError` at 50,000 | Ruff parsed 50,000 and aborted at 100,000. |
| Left-associated `1 + 1 + ...` | CPython `RecursionError` at 50,000 | Ruff parsed 50,000; larger sizes were not checked. |
| Alternating `()[0].x` postfixes | CPython `RecursionError` and Ruff abort at 50,000 | Smaller sizes were not checked in this run. |
| Attribute chain inside f-string or comprehension | CPython `RecursionError` at 50,000 | Ruff parsed 50,000 and aborted at 70,000. |
| Repeated `1 + 1 *` | CPython `RecursionError` at 50,000 | Ruff parsed 50,000 and aborted at 70,000. |
| Long addition inside nested classes | CPython `RecursionError` at 50,000 | Ruff aborted at 70,000 at class depths 0, 20, and 80. |
| Invalid mixed expression with one deleted middle byte | CPython `MemoryError` at generated depth 256 | Ruff returned `syntax-error`; adjacent sampled depths did not reproduce the failure. |

These are observed bounds, not exact thresholds.
The campaign sampled depths rather than searching for a minimum.
Spam rejected each failing source when checked independently.

### Shapes that did not produce a parser failure

At depths up to 5,000, the expanded campaign found no resource failure in unary operators, conditional expressions,
brackets, nested indentation, nested f-string format specs, comprehensions, dictionary unpacking, exception tuples, type
unions, quoted annotations, flat lists, or randomized mixed nesting.
CPython returned controlled syntax errors or syntax limits for some deep brackets, indentation, format specs, mixed
nesting, and nested targets; Ruff often accepted these inputs.

The following larger cases parsed in both implementations: 100,000 comparison links, boolean `or` operands, `match` OR
alternatives, decorators, chained assignments, `with` items, and dictionary unpack entries.
Spam accepted the comparison, boolean, decorator, assignment, `with`, and dictionary cases.
It rejected the 100,000-alternative `match` case, and also rejected a 20,000-term addition that both parsers accepted.
These are conservative rejections worth measuring against real workloads before tightening the AST-depth rule.

## Spam rejections of valid source

In the depth-300 campaign, CPython and Ruff parsed 201 sources that Spam rejected.
Many are ordinary consequences of a depth-100 safety policy: a parser can accept a source well before it becomes unsafe.
The following smaller cases compiled in CPython and parsed in Ruff at the first measured Spam rejection:

| Generated source | Depth parameter | Spam reason | CPython AST depth |
| --- | ---: | --- | ---: |
| Plain string containing 101 `(` characters | 101 | `QuotedAnnotation` | 3 |
| Plain string containing nested annotation-like text | 50 | `QuotedAnnotation` | 3 |
| `case` used as a variable in a `+` chain | 51 | `AstDepth` | 54 |
| Repeated `1 + 1 *` | 51 | `AstDepth` | 54 |
| Repeated `()[0].x` | 34 | `AstDepth` | 105 |
| Actual quoted annotation with nested `list[...]` | 51 | `QuotedAnnotation` | 3 |

The `case` and `1 + 1 *` examples are scanner overcounts: their CPython AST depth is below Spam's configured 100.
The postfix chain has an AST depth above 100, so its rejection is consistent with the configured AST limit.
Quoted annotations may be evaluated later; screening their contents is intentional, though the current estimate is
conservative.
At the time of that run, plain strings and docstrings were also scanned as possible annotations.
That caused clear over-rejection.

A scan of all 654 UTF-8 Python 3.14 standard-library `.py` files under 1 MiB found 214 Spam rejections.
CPython parsed every file, and Ruff parsed all 214 rejected files.
All rejections were `QuotedAnnotation`.
For example, a 116-byte triple-quoted prose string ending in `don't` compiled in CPython and Monty and parsed in Ruff,
but Spam rejected it as `QuotedAnnotation`.
An apostrophe in a long docstring can look like an unterminated string when the scanner interprets its contents as
Python source.
Of the six compact cases above, Monty compiled the three string/annotation examples and rejected the three expression
examples with `SyntaxError` under the sibling checkout used by the probe.

Spam now screens string contents only at likely annotation positions and treats `case` as an operand for chain
estimation, since it can also name a variable at the start of a statement.
The AST estimator also accounts for binary precedence, so `1 + 1 * 1` does not accumulate the depth of two independent
binary chains.
The prose-string, `case`-variable, and 51-term mixed-operator examples above now pass.
A fresh scan accepted all 654 valid Python 3.14 standard-library files under 1 MiB.
A final 83-case database replay rejected all 50 retained parser failures and had no bypass.
Twenty-five retained valid cases remained conservatively rejected, including genuinely deep ASTs, forward annotations,
and deliberately huge flat programs over the token budget.
An earlier 68-case replay under CPython 3.15.0 produced 32 parser failures and no Spam bypass; some previously failing
cases instead parsed or returned controlled syntax errors.
Parser behavior is version dependent.

## Operators and invalid syntax

`tools/make_operator_cases.py` tested 25 binary forms, four unary forms, 13 augmented assignments, and nested assignment
expressions at lengths 80, 256, and 1,024.
It includes arithmetic, matrix multiplication, shifts, bitwise and Boolean operators, comparisons, membership, and
identity tests.
Both parsers handled the generated cases without a resource failure.
Spam rejected 34 sources that both parsers accepted, chiefly long arithmetic and bitwise chains over the configured AST
budget.
Two nested assignment-expression cases hit CPython's controlled syntax limit.
The lexer recognizes the operator tokens needed for screening, but Spam does not validate their semantics or promise
complete Python syntax coverage; augmented assignments and `:=` are screened through their component tokens and
surrounding structure.

`tools/make_invalid_cases.py` made ten single edits to each of 11 source families at three depths.
Of 330 mutated sources, CPython reported 198 syntax errors, 102 controlled syntax limits, 19 successful parses, and 11
`MemoryError`s.
Ruff reported 299 syntax errors and 31 successful parses, without a crash.
The 11 CPython failures were all rejected by Spam.
Most were 3,000-deep power or lambda sources whose base shape already strains CPython; the novel 1,467-byte
mixed-expression mutation failed only at one of the sampled depths.
The absence of a Ruff failure in this bounded campaign is a negative result, not a guarantee for malformed inputs.

## Minimized parser-stack failure

`tools/minimize_case.py` reduced the 1,467-byte `invalid-mixed-delete-middle` CPython failure to a one-byte-minimal
414-byte sequence after testing 51,388 candidates in separate CPython processes.
A trailing newline makes the checked-in reproducer 415 bytes.
CPython 3.14.2 and 3.14.8 both raise `MemoryError: Parser stack overflowed`; CPython 3.15 returns a normal
`SyntaxError`, and Ruff returns a syntax error.
Spam rejects it at AST depth 101.
The survivor contains 122 opening parentheses, 48 opening brackets, 26 opening braces, 17 lambdas, and 13 incomplete
conditional expressions.
This supports the hypothesis that several individually ordinary grammar frames add to one parser-stack budget.
No single exotic token is required.

The minimized source and manifest are in `corpus/minimized/`.
Its parent database case starts with `fd557b88c6d6`.

## Focused attack hypotheses

Three attempts produced concrete bypasses before new rules were added:

| Attack | First observed failure | Why the old screen missed it | Defense |
| --- | --- | --- | --- |
| Flat `if`/`elif` chain | CPython `MemoryError` at 5,966 `elif` clauses on 3.14.2 and 3.14.8; 5,965 on 3.15 | CPython represents flat `elif` syntax as recursively nested `If` nodes. | Track consecutive `elif` clauses at each indentation level as AST depth. |
| Flat f-string fields | 200,000 fields took 6.25 seconds; 300,000 exceeded the five-second campaign limit | There is no lexical nesting, but CPython 3.14 parsing grew roughly quadratically in this range. | Default 100,000-token budget. |
| Repeated invalid `if` tokens | Ruff stack overflow at 173,884 repetitions | Error recovery recursed over malformed flat input while Spam tracked no open grammar frame. | Default 100,000-token budget. |

The post-fix replay blocked all three.
The token budget intentionally rejects some valid giant flat programs, such as 100,000-element literals and argument
lists.
It is configurable because this is a workload policy rather than Python syntax validity.
Applications can additionally configure a maximum token size because one lexical token can still require substantial
parser work.

The next likely attack surfaces are:

1. Other syntax that lowers into recursive AST nodes despite flat source layout.
   `elif` is confirmed; pattern `as`, exception handlers, context-manager items, decorators, match cases, parameters,
   arguments, slices, and comprehension filters were exercised as flat or malformed chains without a parser failure in
   the sampled sizes.
2. High work per lexical token.
   Large hexadecimal integers and Unicode-normalized identifiers remained fast near the source limit.
   The focused pass also accepted and parsed a 780,007-byte string of Unicode name escapes, a 600,006-byte identifier
   with combining marks, a 400,007-byte escaped string, long float fractions, and malformed underscored numbers; none
   took 0.1 seconds in either parser on this machine.
   String escape decoding, Unicode name escapes, numeric conversion, and error-message construction still deserve
   dedicated near-1-MiB cases because a token-count limit cannot see work inside one token.
3. Lexer disagreement around f-string comments, nested format specifications, quote reuse, string prefixes, explicit
   line continuation, and mismatched delimiters.
   A parser may see recursive syntax that the independent lexer treats as text.
   Differential generation should target each lexical state transition, not only grammar productions.
4. Stage-specific failures.
   Ruff failures may occur during AST destruction rather than parsing, while CPython may fail during parsing, AST
   construction, compilation, or cleanup.
   Each stage needs its own probe and timeout.
5. Dynamic parser entry points.
   A short accepted program can construct a large string for `eval()`, `exec()`, or `compile()`.
   Spam must screen the generated source immediately before every parser invocation; screening only the outer module
   cannot address this class.
6. Version and build sensitivity.
   CPython 3.15 changed several malformed-input outcomes, and native stack size affects Ruff abort thresholds.
   Retained cases should run across parser revisions, debug/release profiles, platforms, and constrained memory and
   stack settings.

## Performance

Release builds on macOS arm64, CPython 3.14.2, and `ruff_python_parser` 0.0.14 produced these seven-sample median
in-process times.
Ruff parses and drops an AST; CPython uses `ast.parse`; Spam only screens.
Process startup is excluded.
`tools/benchmark.py` reproduces the inputs and writes raw results to JSON.

| Input | Bytes | Spam | Ruff | CPython |
| --- | ---: | ---: | ---: | ---: |
| `x = 1 + 2` | 10 | 0.3 µs | 0.5 µs | 3.7 µs |
| 80 additions | 326 | 2.9 µs | 7.8 µs | 66.5 µs |
| 10,000 flat assignments | 60,000 | 724 µs | 1,420 µs | 21,758 µs |
| `shutil.py` | 60,568 | 299 µs | 409 µs | 4,953 µs |
| 5,000 additions, Spam rejects early | 20,006 | 3.5 µs | 459 µs | 4,526 µs |

The accepted real-source example adds about 0.30 ms of Spam work before Ruff parsing on this machine.

### Reusable checker and scan specialization (2026-10-24)

After committing limit-based preallocation (`05b7e0e`), release measurements on macOS arm64 with Rust 1.99.0 showed that
constructing scratch storage on every call dominated small-input cost.
The committed binary measured about 1.70 µs for `x = 1 + 2`, 660 µs for 60,000 bytes of flat assignments, and 287 µs for
the installed `shutil.py`.

`Checker::new(limits)` now owns scratch storage across calls.
`Checker::check` resets state but allocates nothing; it does not retain source borrows.
The parser-depth and AST-depth passes specialize on a compile-time flag, and inline hints on token iteration and scanner
observation allow optimization within the screening loop.
The convenience `check` still includes construction and destruction on every call.

The updated `tools/benchmark.py` reports both APIs.
Seven-sample medians from a run with CPython 3.15.0 and the existing Ruff 0.0.14 release probe were:

| Input | Bytes | `check` | Reused `Checker` | Ruff | CPython |
| --- | ---: | ---: | ---: | ---: | ---: |
| `x = 1 + 2` | 10 | 1.7 µs | 0.10 µs | 0.5 µs | 3.9 µs |
| 80 additions | 326 | 3.5 µs | 1.86 µs | 7.9 µs | 69.1 µs |
| 10,000 flat assignments | 60,000 | 540 µs | 553 µs | 1,406 µs | 24,605 µs |
| `shutil.py` | 60,689 | 244 µs | 250 µs | 413 µs | 5,126 µs |
| 5,000 additions, rejected early | 20,006 | 3.9 µs | 2.38 µs | 456 µs | 5,155 µs |

These microbenchmarks include per-call state reset and exclude process startup.
Reused measurements exclude checker construction; the convenience measurements include it.
Small differences between the two APIs on large inputs are within the variation seen between runs.
The installed standard-library source differs from the older measurement above, so compare the optimization against the
same-session committed binary instead.

Reuse reduces tiny-input latency by about 16× relative to the committed allocation baseline.
The larger accepted examples improve roughly 13–16%, but screening still adds about 39% of Ruff's parsing time for flat
assignments and 61% for `shutil.py`.
It is about 2–5% of CPython's `ast.parse` time on these accepted examples.
This is useful progress, not negligible overhead before a fast Rust parser.
Remaining optimization work should profile token classification and the two structural passes on a broader
accepted-source corpus, preserving the existing checks.

Rejection time depends on where the first limit is crossed; the 5,000-addition case stops near its 100th operator.

### Combined scan and bounded simple-prefix path (2026-10-24)

Profiling the previous reusable implementation by stage showed about 180 µs in lexing alone versus 250 µs in full
screening for `shutil.py`.
For flat assignments, lexing took about 262 µs versus 540 µs in full screening.
These isolated stage timings identify costs; they are not additive because optimizing the combined loop changes inlining
and code layout.

The implementation now shares parser/AST structural state with independent depth totals.
It preserves parser rejection priority and checks AST operator depth before the operator can release prefix frames.
A 256-byte static table classifies identifier and numeric continuation bytes.
Ordinary strings search for quote/backslash/newline candidates in a tight loop, comparing triple-quote delimiters only
at quote candidates.

A conservative prefix path counts tokens without constructing them for short, unindented simple lines.
It excludes strings, delimiters, colons, continuations, semicolons, non-ASCII input, and keywords retaining cross-line
state.
Each eligible byte contributes at most one frame to either estimate; requiring the line byte count to fit both budgets
proves that detailed depth accounting cannot reject the line.
Token count and token-size checks remain exact.
Failed attempts leave the lexer position unchanged.
At the first failed line the entire remainder uses the detailed scanner, avoiding repeated fast-path attempts on complex
modules.
Every successfully skipped line leaves structural and annotation state equal to initial state after its newline.

The following final seven-sample medians compare the previous reusable binary against these changes on the same macOS
arm64 environment (Rust 1.99.0, CPython 3.15.0, Ruff 0.0.14).
Construction is excluded from both Spam columns; per-input reset is included.
The Ruff denominator includes parsing and AST destruction.

| Input | Previous reused checker | Current reused checker | Current / Ruff |
| --- | ---: | ---: | ---: |
| tiny | 0.10 µs | 0.03 µs | 5.7% |
| chain-80 | 1.91 µs | 1.67 µs | 21.1% |
| flat-60k | 553.38 µs | 132.74 µs | 9.5% |
| shutil | 250.47 µs | 161.12 µs | 38.4% |
| chain-5000 | 2.36 µs | 2.06 µs | 0.5% |
| ast.py | 109.22 µs | 65.81 µs | 37.8% |
| dataclasses.py | 217.19 µs | 167.37 µs | 43.6% |
| functools.py | 170.72 µs | 117.78 µs | 39.0% |
| json/decoder.py | 54.75 µs | 30.48 µs | 36.7% |
| re/__init__.py | 66.08 µs | 30.34 µs | 40.8% |

All entries except `chain-5000` were accepted by Spam.
The early-rejection result measures rejection latency, not accepted-input overhead.
Reused checks still perform zero heap allocations.
The convenience API, including scratch construction/destruction, measured about 1.2 µs on the tiny input.

The 10% target is met for the tiny expression and flat assignments, but not for general modules.
On the six tested standard-library modules, accepted-input overhead remains roughly 37–44% of Ruff parsing plus
destruction, despite substantial improvement over the previous binary.
Further work needs to reduce lexical work on general Python; combining structural bookkeeping alone cannot achieve the
target when lexing dominates.

Verification included the debug and release test suites, Clippy with warnings denied, and exact result comparisons
against the committed `05b7e0e` checker on all 83 retained corpus cases, 2,051 UTF-8 standard-library files under the
source cap (including malformed fixtures), and 195 generated cases (39 families at depths 1, 10, 80, 101, and 300).
All outcomes, rejection reasons, and byte ranges matched.
Permanent differential tests compare the combined pass with frozen independent-pass rules and compare the prefix path
against detailed scanning across varied budgets.
These checks preserve observed behavior; they do not establish the outstanding parser safety guarantees.

To compare future changes, preserve the previous release benchmark binary before rebuilding, then run:

```sh
python3 tools/benchmark.py \
  --spam-bench target/release/examples/benchmark \
  --baseline /path/to/previous/benchmark \
  --ruff-probe tools/ruff-probe/target/release/spam-ruff-probe \
  --stdlib-file ast.py --stdlib-file dataclasses.py --stdlib-file functools.py \
  --stdlib-file json/decoder.py --stdlib-file re/__init__.py \
  --output /tmp/spam-performance.json
```

### Acceptance-only byte guard prototype (2026-10-24)

The optimized checker was committed as `573d3ca` before this experiment.
`tools/byte_guard.rs` is a separate experimental implementation used only by benchmarks and tests.
It does not change the public library's screening path.

The prototype tracks delimiter matching and indentation directly, bounds parser/AST frames by accumulated syntax
weights, and counts all `elif` occurrences as potentially retained AST depth.
It avoids building tokens and binary-precedence stacks.
It counts string prefixes and paired operator bytes conservatively toward the token budget.
Ordinary literal contents are skipped; possible quoted annotations are proved harmless only when their raw length fits
the conservative remaining depth budget.
Interpolated strings, inconclusive depth bounds, optional token-size limits, malformed lexical structure, and sources
above the default 1 MiB cap fall back to the normal checker.
Both paths allocate their workspaces once and allocate nothing while checking input.

Seven-sample release medians, with construction excluded and per-input reset included:

| Input | Normal reused checker | Byte guard plus fallback | Proof outcome |
| --- | ---: | ---: | --- |
| Tiny expression | 0.03 µs | 0.03 µs | Proved |
| 80 additions | 1.70 µs | 0.64 µs | Proved |
| 60 KB flat assignments | 134 µs | 193 µs | Proved |
| `shutil.py` | 167 µs | 186 µs | Fallback |
| 5,000 additions | 2.07 µs | 2.92 µs | Fallback, rejected |
| `json/decoder.py` | 32.25 µs | 17.11 µs | Proved |
| `re/__init__.py` | 30.07 µs | 50.08 µs | Fallback |

The 80-addition case is about 8% of Ruff's parse/drop time, but `json/decoder.py` is still about 20%.
A second prototype charging identifier bytes instead of classifying keywords was tested and discarded: it did not
improve overall speed and regressed the decoder result.

Coverage checks on the same 2,051 standard-library files proved acceptance for 1,289 files (62.8%).
The other 762 required fallback and were also accepted by the normal checker.
In the 83-case retained corpus, six were proved accepted, two were accepted after fallback, and 75 were rejected after
fallback.
Every positive proof matched normal checker acceptance.
Permanent tests exercise 60,000 seeded malformed sources under varied limits, 975 generated family/size/configuration
combinations, layout boundary cases, and zero-allocation checking of the guard/fallback combination.
The full test suite and Clippy with warnings denied pass.
This is experimental compatibility evidence, not a verified bound on downstream parser behavior.

The whole-file prepass is not enabled by default: late fallback can rescan most of a file, and even successful proofs
can lose to the existing simple-prefix path on flat code.
The next useful experiment is region-level fallback with reusable state at logical-statement boundaries, so one
interpolation does not invalidate an otherwise useful scan of a whole module.
If that cannot materially reduce accepted-module overhead, share lexical work with the downstream parser rather than
adding another independent full-file pass.

Reproduce the experiment by adding `--byte-guard` to `tools/benchmark.py`.
The benchmark executable also accepts `--guard` for timed guard/fallback checking and `--guard-probe` to report `proved`
or `fallback` without timing; positive probe results are asserted against the normal checker.

### Statement-level fallback experiment (2026-10-24)

Committed the whole-file prototype as `874fd61` before starting this experiment.
The `experimental-regions` feature adds the opt-in `Checker::check_regions` method; normal `Checker::check` remains
selected by default.
The experimental implementation lives in `src/lexer/region.rs` and uses the existing lexer context stack and
ordinary-literal scanner, adding no scratch allocations.

At logical statement boundaries, the guard attempts a complete region ending in a root newline.
It counts tokens exactly, validates matching delimiters, and overcounts possible frame additions against the remaining
parser and AST budgets.
Layout changes are emitted by the detailed lexer before another proof is attempted.
Optional token-size limits are checked on the same ranges as the detailed lexer.
A source-byte upper bound eliminates per-token count comparisons when the entire remaining source already fits the
remaining token budget.

Successful proofs clear the current indentation level's elif run only when real code was present; comments and blank
lines preserve it.
Pending lambdas, leading elif/else clauses, semicolons, interpolated strings, and inconclusive annotation/depth bounds
use detailed scanning locally.
Failed proofs restore the lexer's position, contexts and stop flag before scanning the same statement normally.
Subsequent statements can still take the fast path.
The existing simple-prefix optimization remains first, preserving its advantage on flat assignments.

The final release comparison uses a separately built, feature-disabled normal benchmark as the baseline.
Seven-sample medians exclude checker construction and include per-input reset.
This avoids overstating gains: enabling the feature also made the ordinary method slower in the measured binary (for
example, 184 µs on `shutil.py`, versus 159 µs without the feature), although it preserved its results.
Build-dependent timing differences warrant care before making the experimental path a default.

| Input | Normal, feature disabled | Region proofs with local fallback | Regions / Ruff parse and drop |
| --- | ---: | ---: | ---: |
| tiny | 0.03 µs | 0.04 µs | 6.7% |
| chain-80 | 1.65 µs | 0.52 µs | 6.5% |
| flat-60k | 133.85 µs | 128.92 µs | 9.4% |
| shutil | 159.41 µs | 135.06 µs | 33.0% |
| chain-5000 | 2.01 µs | 2.78 µs | 0.6% |
| ast.py | 69.19 µs | 59.14 µs | 34.4% |
| dataclasses.py | 168.32 µs | 150.21 µs | 40.6% |
| functools.py | 117.80 µs | 99.22 µs | 33.1% |
| json/decoder.py | 31.28 µs | 22.69 µs | 27.5% |
| re/__init__.py | 30.58 µs | 23.85 µs | 34.3% |

The general modules improve by roughly 11–27% relative to the feature-disabled baseline, while the early-rejection case
becomes slower.
The broad-module overhead remains 28–41% of Ruff parsing plus AST destruction.
The 10% goal is still met only on the tiny expression, the 80-addition case, and flat assignments in this sample.
The region approach avoids whole-file rescanning, but still performs substantial independent lexical work and some local
rescanning.
It remains opt-in; further work should profile the proof loop and detailed fallback distribution before adding more
lexical machinery.
Sharing tokenization with the downstream parser remains the alternative if this cannot approach 10% on representative
modules.

Verification covers exact results, reasons, and ranges on the 83-case retained corpus snapshot used in this experiment
and 2,051 UTF-8 standard-library files under the source cap.
Feature tests compare 48,000 mixed-statement cases across budgets, 20,000 malformed Unicode sources, 975 generated
family/size/configuration combinations, and quoted-annotation fallback configurations.
Allocation tests cover successful proofs and fallback after failures.
Default and feature-enabled tests, release tests, and Clippy checks pass.
Native annotation recursion and the other safety gaps documented in `SAFETY.md` remain.

To reproduce, first save a release benchmark built without the feature, then build with `cargo build --release
--features experimental-regions --example benchmark` and pass `--regions` to `tools/benchmark.py`.
Its `--baseline` option accepts the saved feature-disabled benchmark.
The executable also accepts `--regions-probe`, which asserts full rejection equality against normal checking and prints
the result without timing.

## Runtime-generated source

The short outer source `eval('1+' * 1000000 + '1')` passed Spam, CPython parsing, and Ruff parsing.
An isolated Monty run exited at its allocator limit while handling the generated expression.
This does not show a Spam bypass for a source passed to Spam: the generated source was never screened separately.
Any integration must check source passed to `eval()` and `exec()` immediately before their parser calls.

## Next experiments

1. Instrument the Ruff probe around parse and drop to locate the stack overflow stage.
   Narrow the first failing depth for each chain with a bounded binary search; record stack limit and build profile with
   the result.
2. Repeat retained cases against newer Ruff revisions and CPython versions.
   Save probe binary hashes and run settings, so a changed result can be attributed to a changed implementation or
   environment.
3. Combine deep expressions with type parameters, `as` and starred patterns, debug f-strings, conversion flags, nested
   format specs, and adjacent string literals.
   The class-body grid did not change the chain thresholds.
4. Extend invalid-source mutation around escaped newlines, CRLF, comments, Unicode identifiers, triple quotes, and exact
   token boundaries.
   Minimize the one mixed-source `MemoryError` and test whether it is stable across versions.

## Source-guided hypotheses

A read-only review of CPython 3.14 and the pinned `ruff_python_parser` 0.0.14 found several paths that the generated
campaigns have not isolated yet.

Ruff guards its normal recursive statement, expression, and pattern parsing with `stacker::maybe_grow` after 20 nested
operations.
Other recursive work remains outside that guard:

- malformed-pattern recovery converts nested `Pattern` values into expressions recursively;
- assignment and deletion target validation recursively walks tuple, list, and starred targets;
- expression-context rewriting recursively walks assignment targets;
- AST visitors recurse through expressions, statements, and patterns;
- IPython help-expression reconstruction recursively walks attributes and subscripts;
- ordinary Rust destruction recursively drops boxed AST chains.

The recovery conversion is used for invalid mapping keys, invalid class patterns, and malformed complex-number patterns.
These deserve late-failing nested and mixed-pattern cases, with parsing and AST destruction observed independently.

Ruff also retains all emitted tokens, including trivia, together with its AST and a vector of parse errors.
It suppresses duplicate errors at one source location but can retain errors at every distinct location.
Long malformed parameter, argument, import, and pattern lists may therefore amplify memory through tokens, AST recovery
nodes, error values, and allocated error strings.

Large literals remain a separate single-token surface.
Ruff removes underscores before converting an integer and then constructs an arbitrary-precision value.
A near-limit decimal integer may be substantially more expensive than the hexadecimal inputs already sampled.
Many unique non-ASCII identifiers may also amplify normalization, interning, hashing, and retained-name memory; repeated
copies of one identifier do not exercise the same path.

CPython's PEG parser runs invalid source in two passes.
The first excludes specialized `invalid_*` rules; after failure it clears memo entries, rewinds, and runs again with
those rules enabled.
Long valid prefixes with a final error can therefore repeat token and grammar work.
Memoized expression, block, pattern, type-parameter, string, and argument rules are promising places to combine delayed
failure with memo-table pressure.

CPython parsing is not the final source-processing stage.
AST preprocessing, symbol-table construction, code generation, control-flow optimization, assembly, and object cleanup
can recurse or allocate after `ast.parse` succeeds.
CPython's own `ast` and `compile` documentation warns that a sufficiently large or complex source can crash during AST
compilation.

Python 3.14 template strings are comparatively underexplored.
Alternating nested f-strings and t-strings, debug fields, conversions, comments, reused quotes, nested format
specifications, and missing final braces exercise lexer state transitions as well as recursive parser paths.

The next source-guided probes should test:

1. Near-limit decimal integers, with and without underscores.
2. Many distinct recoverable errors in parameter, argument, import, and pattern lists.
3. Invalid nested mapping and class patterns that force recovery conversion.
4. Type parameters and argument lists with a long valid prefix and one final invalid token.
5. Many unique identifiers with NFKC expansion.
6. Alternating f-string and t-string nesting, including late malformed format specifications.
7. Successful parse followed separately by AST traversal, destruction, and compilation.
8. Probe strings in less common annotation forms and qualified names to measure the annotation heuristic's remaining
   false positives and blind spots.
   Preserve the zero-rejection result on the 654-file standard-library sample.
9. Run Monty compilation for new retained parser failures and exercise every dynamic parser entry point.
   The SQLite corpus currently records Monty only for the initial deep power and lambda cases.

### Late invalid call arguments

The CPython two-pass hypothesis produced a reproducible parser-budget failure that Spam accepts with its default limits.
The compact form is:

```python
f(x,x,x, ... 49,000 arguments ...,x,if)
```

The source is 98,006 bytes and about 98,000 tokens.
It remains below Spam's 1 MiB source and 100,000-token defaults, has no deep nesting, and was accepted by the release
checker.
The final `if` makes an otherwise valid call invalid only after the parser has consumed the long argument prefix.

Single isolated parse measurements on macOS arm64 were:

| Parser | Valid repeated `x` | Final invalid `if` | Invalid child peak RSS |
| --- | ---: | ---: | ---: |
| CPython 3.14 | 136 ms | 254 ms | 176 MB |
| CPython 3.15 | 181 ms | 242 ms | 177 MB |
| Ruff 0.0.14 | 19 ms wall | 40 ms wall | 12 MB |

`resource.getrusage(RUSAGE_CHILDREN)` reports whole-child peak RSS, including interpreter startup; it is not an
incremental parser-allocation measurement.
The CPython timings are measured inside `ast.parse`, while the Ruff values include subprocess startup.
Repeated literal `0` arguments remove identifier handling and still take 210 ms with 130 MB peak RSS on CPython 3.14.
This isolates the long argument list and late failure as the important shape.

With distinct argument names, CPython 3.14 crossed the proposed 100 ms parse budget between 20,000 and 30,000 arguments:

| Arguments | Source | Valid parse | Final invalid `if` | Invalid child peak RSS |
| ---: | ---: | ---: | ---: | ---: |
| 10,000 | 59 KB | 13 ms | 34 ms | 51 MB |
| 20,000 | 129 KB | 29 ms | 76 ms | 90 MB |
| 30,000 | 199 KB | 44 ms | 128 ms | 116 MB |
| 40,000 | 269 KB | 55 ms | 172 ms | 155 MB |
| 49,000 | 332 KB | 68 ms | 216 ms | 183 MB |

The late invalid token roughly doubles or triples parse time and substantially raises peak memory.
This is consistent with CPython discarding first-pass memoization and reparsing with invalid-syntax rules, although the
probe does not attribute individual allocations.
No tested process crashed or timed out under the 500 ms discovery timeout.

This is a concrete accepted input that exceeds a 100 ms parser target and is a likely 64 MiB incremental-memory
violation at larger sizes.
Any permanent regression case should record the implementation, version, source generator, timeout, and resource
threshold rather than checking only for a crash.

The same campaign found no Ruff failure in the sampled ranges.
Near-limit decimal literals parsed in roughly 13 ms at 1 MiB, and nested malformed mapping/class patterns plus recursive
assignment and deletion targets remained below roughly 15 ms through depth 20,000.
Spam rejected those deep recursive forms near its nesting limit, before Ruff saw them.
Twenty-four thousand unique NFKC identifiers stayed below 100 ms in CPython; 30,000 crossed 100 ms but Spam rejected
that case on token count.
Large flat template-string campaigns were also rejected on token count before reaching the parser budget.

Keep generated campaigns bounded by the subprocess timeout and 1 MiB source cap.
Import reproducible failures and representative conservative rejections into SQLite; add negative results and new
hypotheses to this log so the next campaign explores different grammar paths.

## Configuration-bound fuzzing

After annotation scanning became iterative, every allocation-driving setting received a hard implementation ceiling.
The source fuzz target now derives all public settings from its input, including optional token, AST, and experimental
construct-width limits.
Each value can be zero, within its supported range, disabled where applicable, or one above its hard ceiling.
Invalid configurations must fail before scratch allocation.

A 30-second libFuzzer and AddressSanitizer run on macOS arm64 completed 1,671,362 executions without a crash or
sanitizer finding.
It reached 1,115 coverage edges and 6,145 feature edges, retained about 1,600 corpus entries, and peaked at 551 MiB RSS
under ASan.
Generated inputs were capped at 4 KiB by libFuzzer for this first run.
Longer campaigns should seed valid Python and nested annotation forms so more executions reach the detailed scanner
instead of configuration validation and UTF-8 rejection.

The outer source and nested annotation scans now also share cumulative byte and token work budgets.
Reinterpreting a quoted annotation charges its contents again, preventing configured work from being multiplied by the
annotation-depth limit.
Replaying the 2,778-file calibration corpus produced no new rejection, and the 101 retained adversarial cases kept their
previous classifications.

## Construct-width differential oracle

The construct-width oracle uses CPython's AST as an independent reference.
For each metric and source, it configures only that Spam limit, verifies acceptance at CPython's measured maximum, and
verifies the matching rejection one below it.
The first suite covers 157 boundaries across 27 sources, including unpacking, positional-only and keyword-only
parameters, comprehensions, grouping, class bases, parenthesized imports, starred slices, adjacent strings, nested
f-strings, format fields, and template strings when supported.

The first run found 14 mismatches:

- `/` and bare `*` parameter separators were counted as parameters;
- class bases and parenthesized imports were classified as calls;
- grouping and comprehensions were classified as displays;
- a single subscript item was classified as a slice tuple;
- tuple slices were also counted as displays by the calibration oracle;
- interpolation fields accumulated across unrelated strings and across nested format levels.

Spam now distinguishes those lexical roles.
Adjacent string literals still accumulate interpolation fields because CPython folds them into one `JoinedStr`, while
nested format levels and nested f-strings keep independent maxima.
The calibration walker no longer counts a tuple used as `Subscript.slice` as a display.
After these corrections, all 157 boundaries matched CPython.

## Region cost profile

`tools/profile_regions.py` instruments a committed snapshot in a temporary checkout, leaving production builds free of
profiling branches.
It samples every sixteenth proof, rotates the sampling phase, and subtracts calibrated clock bookkeeping.
Stage shares are estimates; reports include instrumented wall time to expose measurement overhead.
Tiny inputs have too much relative overhead for reliable stage attribution.

The retained `campaigns/2026-10-24-region-profile/profile.json` measures `a91af29` on macOS arm64.
Across six standard-library files, successful proofs accounted for about 36–54% of measured time, detailed scanning and
dispatch for 38–53%, and failed proofs for 7–10%.
Annotation rescanning was negligible in those files; the synthetic annotation workload spent about 76% in child scans.
Instrumented wall time was 10–14% higher on these modules.
These results identify broad cost centers, not exact instruction costs or parser-safety evidence.

Keeping a frame's lexer live until an annotation suspends it removes per-token state saves and reconstruction.
A controlled comparison on `e236a65`, using the profiler's `--live-lexer` experiment, reduced the `shutil.py` region
check from about 150 to 139 microseconds.
The current scanner includes this change.
Absolute results vary between runs; this is a modest improvement, not the desired 10% parser-overhead result.

Failed proofs are too small a share to explain the remaining gap.
A promising next experiment is sharing the downstream parser's lexical pass.
Inspection of Ruff 0.0.14 shows that its lexer constructor and token-source driver are private; public parsed tokens are
available only after parsing.
A real integration needs an early-abort observer plus correct handling of token checkpoints, rewinds, re-lexing, and
separate quoted-annotation scans.
Replaying already parsed tokens could estimate a cost floor, but would not demonstrate protection before AST
construction.

## Generated differential, parser, and sanitizer campaigns

On 2026-10-24, a deterministic mixed-grammar differential checked 1,000 CPython 3.15 programs against Ruff 0.0.14.
For each program, Spam was tested at CPython's exact call, parameter, display, slice, and interpolation width and one
below it.
There were no mismatches.
A separate 100-case run through Pyrefly 1.4.0 had no process failure.

A parser attack run tested 851 generated sources up to depth 100,000 with one-second subprocess deadlines.
CPython or Ruff failed in 89 cases; Spam rejected every failing case.
The SQLite importer retained 50 new shortest shapes and 12 known cases as run 31.
Stable boundary searches found CPython `MemoryError` on 5,966 unary operators, CPython `RecursionError` on a
95,080-member f-string attribute chain, and a Ruff process crash on a class pattern nested 86,959 levels.
Ruff format-spec timeouts were not monotonic in adjacent controls and remain a timing direction rather than a minimized
failure.

The refreshed Python 3.15 calibration measured 2,955 valid files from the standard library, Monty, and Pydantic.
Spam and Ruff accepted all of them.
The largest observed widths were 154 call arguments, 66 parameters, 2,231 display items, 49 slice items, and 181
interpolation fields.

Two 120-second sanitizer runs used valid grammar seeds and source-plus-configuration seeds, including nested quoted
annotations.
The grammar target grew to 1,052 edges and 4,640 features in the captured run; the source target executed about 2.2
million inputs and reached 1,194 edges and 7,331 features.
Neither run produced a crash or sanitizer report.

## Final stabilized-width benchmark

The release benchmark after making width settings always available measured a reused checker at 0.03 µs for the tiny
case, 2.97 µs for an accepted 80-member chain, 135.46 µs for a 60 KiB flat source, 226.62 µs for `shutil.py`, and 4.32
µs for an early rejection of a 5,000-member chain.
The accepted flat source took 9.7% of Ruff's parse time; the larger `shutil.py` input took 54.0%.
Results are in `campaigns/2026-10-24-final-benchmark.json` and remain specific to this machine and build.
