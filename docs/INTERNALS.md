# How Tokspam scans source

Tokspam estimates structural pressure without building an AST. It keeps just the
state needed to recognize expensive shapes and compare them with configured
limits. This guide explains the implementation; [SAFETY.md](SAFETY.md) records
the bounds and the limits of the safety claims.

## Follow the source

`src/lib.rs` reexports the public API. `src/limits.rs` defines optional thresholds,
`src/scores.rs` defines reports and storage policy, and `src/checker/mod.rs` owns
the scan loop. Analysis specializes the loop to collect scores; checking compiles
recording out and can use acceptance proofs.

The checker rejects oversized source before initializing its scratch buffers.
It then consumes the streaming lexer one token at a time. Tokens refer to byte
ranges in their input: the lexer does not decode strings or collect a token list.
Only escaped plain annotation strings need decoded scratch before a child scan;
ordinary source and unescaped annotations remain borrowed.

Each token passes through shared work budgets, lexical checks, construct-width
tracking, structural rules, and annotation-position tracking. This order matters:
for example, malformed delimiters must be rejected before other trackers rely on
their structure. It also determines which rejection is reported when several
limits would fail at the same token.

## Estimate depth without constructing a tree

`src/rules.rs` tracks open structural levels and operator state. Parser-depth
accounting estimates pending recursive work. AST-depth accounting combines
operand heights according to operator precedence: a binary node adds one to the
larger child height, while independent sibling expressions do not accumulate.
Scalar totals avoid repeatedly walking every open level for each token.

`src/checker/widths.rs` counts items such as arguments and display elements during
analysis or when their optional limits are enabled. With those limits disabled,
checking omits width tracking altogether. Unfinished lambda parameter lists may
retain frames before charging parser depth; the width tracker uses the same
fallible growing or fixed storage as the rest of the scan.

These are conservative lexical estimates, not a full Python grammar. Acceptance
does not validate syntax or guarantee that every downstream parser is safe.

## Quoted types use the same buffers

`src/checker/annotations.rs` identifies likely annotation positions. It excludes
value positions such as `Literal` operands and `Annotated` metadata by lexical
name; it does not resolve imports or aliases.

A quoted type may contain another expression that needs screening. Rather than
recursively calling the checker, the scan loop suspends its current frame and
pushes a child onto an explicit work stack. The child receives the remaining
depth budget. Bytes and tokens are charged to shared cumulative budgets, so
nested strings cannot each obtain a fresh allowance.

`src/checker/scratch.rs` saves scalar state and marks in the shared stacks.
Parent entries remain below a segment boundary; the child uses the space above
it. Returning discards the child's entries and restores the parent boundary.
This avoids copying stacks or allocating a separate set for each annotation.
Marks are indices, so growing a backing vector cannot invalidate them.

Rejections inside quoted types point to the enclosing annotation in the original
source. Exhausting the optional work budget returns `Inconclusive`; scratch exhaustion
returns `Storage`. Neither establishes a complexity violation.

`src/checker/escapes.rs` decodes escaped annotations into one shared text arena.
Frames retain ranges and allocation marks; nested decoding can append without
invalidating indices into parent text. Each append preserves UTF-8, so resuming
needs no validation pass over the whole parent. Decoding and subsequent scanning
are both charged to the work budget. Unsupported named Unicode escapes and
isolated surrogates return `AnnotationEscape` instead of a guessed value.

## Reuse and allocation

Every check resets logical state, including after rejection, while retaining
allocated capacity. `Checker::new` grows buffers fallibly as needed.
`Storage::Fixed { capacity }` reserves an explicit entry count per stack at
construction, so input cannot trigger allocations during scanning. Exhaustion
returns an incomplete result. Neither mode retains source strings.

## Keep the common path small

The lexer separates ordinary code, string scanning, indentation, and byte-search
helpers under `src/lexer/`. Searches over literal text use `memchr` where useful.
Shortcuts for simple lines, and optional statement-region proofs, skip detailed
tracking only when they can establish the relevant bounds. Uncertain input falls
back to token scanning.

The scan loop uses const generics to compile out disabled shortcuts and unused
width counting. Mode selection happens once per input rather than adding flags
to every token's work. The lexer stays live until an annotation suspends it,
avoiding repeated save/restore work on ordinary input.

Allocation regressions live in `tests/allocations.rs`; detailed-scan comparisons
live beside the checker. Corpus comparisons and release benchmarks belong to
the separate `tokspam-fuzzer` repository. Refactoring hot paths requires both
behavior comparisons and measurements: equivalent Rust source can still change
inlining and generated code.
