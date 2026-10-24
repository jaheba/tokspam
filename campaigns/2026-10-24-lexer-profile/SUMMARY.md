# Lexer profiling and optimizations

Baseline: `f14382033be22a06098e9f5a29bb7971b05d7882`, release build on macOS arm64.
The candidate changes `src/lexer.rs` to scan runs of interpolated-string text
and horizontal whitespace, and moves interpolated-text handling out of the
ordinary token dispatch loop. No dependencies, unsafe code, new allocations, or
resource-limit changes were added.

## Profile

The isolated lexer benchmark consumes every token with `black_box`, reusing
scratch storage. It does not use the simple-line or experimental region proofs.
Its cost is therefore distinct from the fraction of complete-check time spent
lexing, and includes the benchmark's token consumption overhead.

macOS `sample` profiles are retained as `sample-fstrings.txt` and
`sample-stdlib.txt` (binary-image inventories omitted). In the long-text f-string
workload, 748 of 2,232 samples were in or calling `memcmp`, due to repeated
variable-width quote comparisons. The baseline traversed literal text one Unicode
scalar at a time through the full context dispatch. In `shutil.py`, indentation
accounted for 351 of 2,463 samples and ordinary string scanning for 226 samples;
most remaining samples were in the inlined token loop. These sampling results
identify hot paths, not exact per-operation timings.

## Changes

- Scan interpolated text up to the next quote, brace, backslash, or EOF in a byte
  loop. Format text needs only braces and backslashes. ASCII stopping points
  preserve UTF-8 boundaries, including malformed or unterminated input.
- Test the quote byte before comparing a triple delimiter. Single quotes need
  no variable-length comparison.
- Keep interpolated-text processing in a separate non-inlined helper. An initial
  version expanded the ordinary token loop and slowed typical lexing; extraction
  removed that regression in the representative module measurements.
- Consume horizontal whitespace runs without repeating the outer lexer dispatch.
  Indentation, tabs, form feeds, and line continuations retain their original
  handling.

## Results and limits

`benchmark.json` records two rounds in opposite binary orders; each observation
is itself a seven-sample median. On six standard-library modules, isolated lexer
time fell by 3–8%, while complete reusable-checker time fell by about 0–2%.
The synthetic long-text f-string case used 16% of the original lexer time and
18% of complete-check time. The long-space case used 35% of lexer time and 37%
of complete-check time. Identifier-heavy and ordinary-string complete checks
were about 1% slower or unchanged. Small differences can reflect measurement
noise and code-layout effects; the large synthetic improvements do not represent
a general 6× speedup or establish parser safety.

Exact token kinds, byte ranges, and string-content ranges matched the baseline
on all 3,205 inputs: 1,000 deterministic malformed Unicode strings, 2,054
standard-library files, and 151 retained corpus cases. `differential.json`
records this comparison. Focused unit tests cover Unicode text, doubled braces,
escaped quotes, triple delimiters, template strings, nested format fields,
unterminated text, and whitespace/indentation ranges.

Default and all-feature tests and Clippy checks passed, including the allocation
regression test. See the README for reproduction commands using
`tools/profile_lexer.py` and `tools/check_lexer_equivalence.py`.
