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

After committing limit-based preallocation (`05b7e0e`), release measurements on macOS arm64 with Rust 1.99.0 showed
that constructing scratch storage on every call dominated small-input cost. The committed binary measured about
1.70 µs for `x = 1 + 2`, 660 µs for 60,000 bytes of flat assignments, and 287 µs for the installed `shutil.py`.

`Checker::new(limits)` now owns scratch storage across calls. `Checker::check` resets state but allocates nothing;
it does not retain source borrows. The parser-depth and AST-depth passes specialize on a compile-time flag, and
inline hints on token iteration and scanner observation allow optimization within the screening loop.
The convenience `check` still includes construction and destruction on every call.

The updated `tools/benchmark.py` reports both APIs. Seven-sample medians from a run with CPython 3.15.0 and the
existing Ruff 0.0.14 release probe were:

| Input | Bytes | `check` | Reused `Checker` | Ruff | CPython |
| --- | ---: | ---: | ---: | ---: | ---: |
| `x = 1 + 2` | 10 | 1.7 µs | 0.10 µs | 0.5 µs | 3.9 µs |
| 80 additions | 326 | 3.5 µs | 1.86 µs | 7.9 µs | 69.1 µs |
| 10,000 flat assignments | 60,000 | 540 µs | 553 µs | 1,406 µs | 24,605 µs |
| `shutil.py` | 60,689 | 244 µs | 250 µs | 413 µs | 5,126 µs |
| 5,000 additions, rejected early | 20,006 | 3.9 µs | 2.38 µs | 456 µs | 5,155 µs |

These microbenchmarks include per-call state reset and exclude process startup. Reused measurements exclude
checker construction; the convenience measurements include it. Small differences between the two APIs on large
inputs are within the variation seen between runs. The installed standard-library source differs from the older
measurement above, so compare the optimization against the same-session committed binary instead.

Reuse reduces tiny-input latency by about 16× relative to the committed allocation baseline. The larger accepted
examples improve roughly 13–16%, but screening still adds about 39% of Ruff's parsing time for flat assignments
and 61% for `shutil.py`. It is about 2–5% of CPython's `ast.parse` time on these accepted examples. This is useful
progress, not negligible overhead before a fast Rust parser. Remaining optimization work should profile token
classification and the two structural passes on a broader accepted-source corpus, preserving the existing checks.

Rejection time depends on where the first limit is crossed; the 5,000-addition case stops near its 100th operator.

### Combined scan and bounded simple-prefix path (2026-10-24)

Profiling the previous reusable implementation by stage showed about 180 µs in lexing alone versus 250 µs in
full screening for `shutil.py`. For flat assignments, lexing took about 262 µs versus 540 µs in full screening.
These isolated stage timings identify costs; they are not additive because optimizing the combined loop changes
inlining and code layout.

The implementation now shares parser/AST structural state with independent depth totals. It preserves parser
rejection priority and checks AST operator depth before the operator can release prefix frames. A 256-byte static
table classifies identifier and numeric continuation bytes. Ordinary strings search for quote/backslash/newline
candidates in a tight loop, comparing triple-quote delimiters only at quote candidates.

A conservative prefix path counts tokens without constructing them for short, unindented simple lines. It excludes
strings, delimiters, colons, continuations, semicolons, non-ASCII input, and keywords retaining cross-line state.
Each eligible byte contributes at most one frame to either estimate; requiring the line byte count to fit both
budgets proves that detailed depth accounting cannot reject the line. Token count and token-size checks remain
exact. Failed attempts leave the lexer position unchanged. At the first failed line the entire remainder uses the
detailed scanner, avoiding repeated fast-path attempts on complex modules. Every successfully skipped line leaves
structural and annotation state equal to initial state after its newline.

The following final seven-sample medians compare the previous reusable binary against these changes on the same
macOS arm64 environment (Rust 1.99.0, CPython 3.15.0, Ruff 0.0.14). Construction is excluded from both Spam columns;
per-input reset is included. The Ruff denominator includes parsing and AST destruction.

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

All entries except `chain-5000` were accepted by Spam. The early-rejection result measures rejection latency,
not accepted-input overhead. Reused checks still perform zero heap allocations.
The convenience API, including scratch construction/destruction, measured about 1.2 µs on the tiny input.

The 10% target is met for the tiny expression and flat assignments, but not for general modules. On the six tested
standard-library modules, accepted-input overhead remains roughly 37–44% of Ruff parsing plus destruction, despite
substantial improvement over the previous binary. Further work needs to reduce lexical work on general Python;
combining structural bookkeeping alone cannot achieve the target when lexing dominates.

Verification included the debug and release test suites, Clippy with warnings denied, and exact result comparisons
against the committed `05b7e0e` checker on all 83 retained corpus cases, 2,051 UTF-8 standard-library files under the
source cap (including malformed fixtures), and 195 generated cases (39 families at depths 1, 10, 80, 101, and 300).
All outcomes, rejection reasons, and byte ranges matched. Permanent differential tests compare the combined pass
with frozen independent-pass rules and compare the prefix path against detailed scanning across varied budgets.
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
5. Probe strings in less common annotation forms and qualified names to measure the annotation heuristic's remaining
   false positives and blind spots.
   Preserve the zero-rejection result on the 654-file standard-library sample.
6. Run Monty compilation for new retained parser failures and exercise every dynamic parser entry point.
   The SQLite corpus currently records Monty only for the initial deep power and lambda cases.

Keep generated campaigns bounded by the subprocess timeout and 1 MiB source cap.
Import reproducible failures and representative conservative rejections into SQLite; add negative results and new
hypotheses to this log so the next campaign explores different grammar paths.
