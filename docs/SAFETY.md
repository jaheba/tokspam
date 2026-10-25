# Tokspam safety requirements

Tokspam processes untrusted Python source before the selected Python parser runs.
The source may be malformed and may be constructed to exhaust CPU time, memory, or the native call stack.
Tokspam must return acceptance or a rejection without panicking or aborting the process.

## Scope

Tokspam protects the availability of a downstream parser and AST pipeline.
It does not classify program behavior or reject imports, calls, builtins, filesystem access, network access, or other
runtime capabilities.
Those controls belong to the interpreter and its host.

## No recursion

Production code must not recurse over source structure.
This includes Python delimiters, indentation, f-string fields, format specifications, expressions, annotations, and
malformed recovery paths.
Source nesting may increase the size of an explicit heap-backed work stack, subject to a hard internal limit, but
must not increase the native Rust call-stack depth.

The lexer, structural scanners, and quoted-annotation scanner are iterative.
Quoted annotations use a bounded explicit work stack with one frame per permitted annotation level.
Suspending an outer scan retains scalar tracking state and leaves its live stack entries in shared backing buffers.
The child uses a new segment above those entries; returning truncates the child segment and restores the parent.
Children inherit the remaining parser-depth budget, so live delimiter and indentation entries across scans are bounded
by the outer nesting budget, plus per-frame roots and transient overflow entries.
Annotation tracking distinguishes the type operand of `Annotated` from its metadata, and `Literal` values from type
operands. Strings in those value positions are not rescanned; the containing expressions still pass through the ordinary
structural scanner and shared token and work budgets. This distinction uses lexical names without resolving aliases.
Malformed delimiters are rejected by the lexer before they can desynchronize these structural stacks.

## Resource bounds

Every public limit has a smaller-or-equal hard implementation ceiling.
A caller may tighten a limit but may not configure an unbounded source size, token count, token size, scan-work budget,
nesting depth, indentation depth, annotation depth, or work-stack size.
Configuration outside those ceilings must be rejected before source scanning begins.

For accepted configuration, one call to `check` must have these bounds:

- Native stack use is constant with respect to source structure.
- Working memory is bounded by explicit nesting, indentation, operator, and annotation limits.
- Ordinary scanning is linear in source bytes and emitted tokens.
- Annotation contents are charged to one shared byte and token budget when scanned as Python source.

The implementation uses seven shared `Vec` scratch buffers and a bounded work-frame `Vec`.
Capacities are determined by configured nesting, indentation, and annotation limits, including per-frame roots and
transient overflow entries. The width tracker reserves its hard capacity for unfinished lambda parameter lists, which
can accumulate before charging parser depth. Capacities remain bounded by hard implementation ceilings.
Internal field widths retain the full hard-limit range: stack marks use `u16` (capacity at most 65,535), saved binary
operand depths use `u16` (the AST ceiling plus at most six precedence folds), and construct counts use `u32` (bounded
by the shared token budget). Indentation columns use `u32`; each source byte adds at most eight columns, including tabs.
Compile-time assertions protect these relationships when hard ceilings change. Parser totals, pending lambda counts,
and depth arithmetic remain `u32`; only bounded saved values are narrowed. Separate precedence and depth arrays avoid
per-entry tuple padding without changing the six-entry operator stack.
AST accounting keeps the current operand's postfix height separate from saved binary left operands. A fixed-size
precedence fold combines subtree heights by maximum plus the operator node. Closing a child level preserves its peak
height in its parent operand; separators within the child retain the maximum item rather than summing siblings.
While visiting a child, the parent contributes pending binary ancestors and any conservative postfix wrapper charge,
instead of adding the already checked heights of sibling left subtrees. The parent expression contribution is restored
and folded with the child's peak on return. This updates scalar totals without walking the open-level stack.
The fold examines at most six entries per token, and the extra scalar per structural level does not add allocations.
Unfinished lambda parameter lists can retain width frames before charging parser depth. Exhausting the fixed width
buffer returns `Inconclusive`, because a structural violation has not necessarily been established.
When all optional width budgets are disabled, capacity-only tracking preserves the
same frame openings, parameter closures, and shared hard-capacity rejection without
maintaining unused item counts. The mode is selected once for each input.
`Checker::new` starts with empty vectors and grows them on demand up to configured
ceilings. Allocated capacity may depend on input in this mode, but its upper bound
does not. Explicit geometric reservation caps every growth request at its ceiling.
`Checker::preallocated` reserves that complete storage at construction; scanning
then performs zero heap allocations, with allocation count and sizes determined
only by configuration. Both modes assert logical push bounds before appending.
Rejection does not leave segment bases active for the next check.
The allocation regression tests cover construction and accepted and rejected input under several configurations.
`Limits::validate` and `Checker::new` reject settings above the hard ceilings; the one-shot `check` API returns a
configuration rejection and rejects oversized source before constructing its checker.
Optional limits remain bounded by the hard source and token ceilings when disabled.

The outer source and every nested annotation scan share cumulative byte and token work budgets. `max_work_units` also
bounds their sum and reports `Inconclusive` when the checker runs out of fuel without identifying a structural limit.
An annotation range is charged again when its contents are interpreted as Python source, so configured work cannot be
multiplied by annotation depth.

The source string is owned by the caller and is not counted as Tokspam working memory.

## Malformed input

Malformed input follows the same resource limits as valid input.
Lexer errors, mismatched delimiters, unterminated strings, invalid UTF-8 boundaries in token ranges, and incomplete
f-strings must not cause a panic, an unbounded loop, or repeated scanning outside the shared budgets.
All returned byte ranges must be valid boundaries in the input `str`.

Literal-text searches use the safe `memchr` APIs after a short scalar prefix. Tokspam contains no hand-written SIMD
or unsafe loads. The dependency owns platform-specific acceleration and fallbacks. Searches for four delimiters combine
`memchr3` and `memchr` within fixed 64-byte blocks: a repeated fourth delimiter cannot trigger unbounded rescanning of
the remaining literal. Matches are ASCII boundaries or EOF, preserving UTF-8 source ranges. The wrapper allocates
nothing, does not decode literals, and leaves shared work accounting unchanged.

## Verification

The safety checks require:

1. A fuzz target that varies both arbitrary UTF-8 source and every public configuration field.
2. Regression cases for maximum permitted stacks and values immediately above each limit.
3. Tests that catch panics and verify returned byte ranges for malformed Unicode input.
4. Scaling tests at increasing input sizes for each generator family.
   Doubling source size should not produce quadratic Tokspam runtime.
5. Memory and stack constrained runs of the retained corpus on supported platforms.
6. A host deadline around Tokspam and the downstream parser.
   The deadline remains the final availability limit if an implementation bug violates an expected complexity bound.

The 2026-10-24 sanitizer campaign ran both fuzz targets for 120 seconds with valid grammar, nested annotation, and
varied-configuration seeds.
It found no crash or sanitizer report.
Release candidates still require longer runs on every supported platform.
