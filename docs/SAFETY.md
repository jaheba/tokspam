# Resource and measurement requirements

Tokspam processes untrusted Python source without executing it or building an AST.
Scores model lexical structure; completion is not a verified guarantee of safe
parsing or proof that the source is valid Python.

## Iterative scanning

Production code must not recurse over input structure. The lexer, structural
trackers, and quoted-annotation work loop use explicit heap-backed stacks.
Parents retain scalar state and entries below a segment boundary; children borrow
the remaining structural budget and use entries above that boundary. Restoring a
parent discards the child segment without copying stacks.

Quoted annotations share cumulative token and byte/work accounting. Plain escaped
annotation strings are decoded once per annotation level into a shared UTF-8 text
arena; unescaped strings remain borrowed. Raw and byte literals keep their existing
semantics. No source is executed. `Literal` operands and `Annotated` metadata are
excluded using lexical names, without resolving aliases.

The text arena is fallible and obeys the same storage choice as the eight stacks.
Fixed storage reserves the selected capacity in bytes for this ninth scratch buffer.
Parent text is append-only while a child runs; saved indices survive relocation,
and returning truncates only bytes allocated by the finished frame. Each append
preserves UTF-8, avoiding unsafe conversion or repeated full-string validation on
resume. Decoder failures stop before any child frame can use partial output.

Charge the raw decoding pass and the decoded-source scan separately to shared
work accounting. Decoding does not expand byte length; it cannot evade the byte
bound used by structural-only shortcuts. Quotes, backslashes, control escapes,
octal, hex, and numeric Unicode escapes are supported. Unknown escapes preserve
their backslash. Named Unicode escapes require a name database, and isolated
surrogates cannot inhabit Rust str; both return `AnnotationEscape` without guessing
or substituting characters.

## Storage and arithmetic

There are no arbitrary configuration ceilings. Scores and stack marks use `usize`;
representation and available memory remain finite. Growing stacks use fallible
reservation; fixed stacks reserve caller-selected capacity and never allocate
while scanning. No input-triggered native recursion is permitted in either mode.
Storage exhaustion must return an incomplete result, not panic or claim that a
complexity threshold was exceeded. Failed pushes must stop before code relies on
a missing entry. Fixed reservation failure is reported by the constructor.

Structural accounting has a constant maximum charge per scanned byte. Before
scanning, and before scheduling a quoted-type rescan, the checker verifies that
sixteen times cumulative scanned bytes is representable. This conservative
arithmetic check bounds counters, indentation column expansion, and structural
charges without wrapping. It reports `Overflow`, not a score violation. Cumulative
byte addition is also checked. These representational checks replace the former
small source/depth ceilings; they are not configurable complexity policies.

Working memory grows with simultaneously retained structure in growing mode. Fixed
mode has constant working memory with respect to input. Caller-owned source bytes
are outside scratch accounting. Reuse resets all logical segments after success
or interruption and retains capacity.

## Scores, limits, and shortcuts

Analysis collects observed maxima through detailed token scanning. Checking may
compile out score recording, unused width tracking, and disabled paths. A shortcut
must preserve acceptance/rejection and relevant thresholds; it cannot silently
skip annotation work that affects a configured token or work budget.

Reported depths are heuristic estimates. AST accounting combines sibling subtree
heights by maximum plus an operator node and keeps a fixed six-entry precedence
fold per structural level. This fixed array represents the six modeled precedence
classes; it is not a configurable input-depth ceiling.

Malformed and Unicode input must terminate with valid UTF-8 source ranges. Invalid
or unsupported lexical structure stops measurement explicitly. Nested errors refer
to an enclosing range in the original source. No incomplete report may present
its partial maxima as completed measurements.

## Validation

Run `cargo test`, all-feature regressions, and
`cargo clippy --all-targets --all-features -- -D warnings` after accounting changes.
Tests must cover optional/zero/large thresholds, growing and fixed storage,
post-failure reuse, score boundaries, and checking/analysis agreement. Benchmark
release builds against a saved baseline when changing the token loop.

Parser comparisons, corpus generation, fuzzing and benchmark campaigns belong to
`tokspam-fuzzer`. Earlier campaign outcomes apply to their recorded revisions and
conditions, not automatically to this measurement API or arbitrary parser versions.
Long-running fuzzing and platform-specific parser campaigns remain release work.
