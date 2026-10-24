# Spam safety requirements

Spam processes untrusted Python source before the selected Python parser runs.
The source may be malformed and may be constructed to exhaust CPU time, memory, or the native call stack.
Spam must return acceptance or a rejection without panicking or aborting the process.

## Scope

Spam protects the availability of a downstream parser and AST pipeline.
It does not classify program behavior or reject imports, calls, builtins, filesystem access, network access, or other
runtime capabilities.
Those controls belong to the interpreter and its host.

## No recursion

Production code must not recurse over source structure.
This includes Python delimiters, indentation, f-string fields, format specifications, expressions, annotations, and
malformed recovery paths.
Source nesting may increase the size of an explicit heap-allocated work stack, subject to a hard internal limit, but
must not increase the native Rust call-stack depth.

The lexer, structural scanners, and quoted-annotation scanner are iterative. Quoted annotations use a preallocated work
stack with one frame per configured annotation level. Suspending an outer scan stores scalar lexer state; delimiter,
indentation, parser-depth, and AST-depth state remain in that frame's preallocated scratch storage.

## Resource bounds

Every public limit has a smaller-or-equal hard implementation ceiling.
A caller may tighten a limit but may not configure an unbounded source size, token count, token size, nesting depth,
indentation depth, annotation depth, or work-stack size.
Configuration outside those ceilings must be rejected before source scanning begins.

For accepted configuration, one call to `check` must have these bounds:

- Native stack use is constant with respect to source structure.
- Working memory is bounded by explicit nesting, indentation, operator, and annotation limits.
- Ordinary scanning is linear in source bytes and emitted tokens.
- Annotation contents are charged to one shared byte and token budget when scanned as Python source.

The current implementation preallocates scratch and work-stack storage for every permitted annotation depth before token
scanning. Its heap allocation count and sizes depend only on `Limits`; scanning does not grow those allocations.
Oversized source is rejected before this allocation step.
The allocation regression test covers accepted and rejected input under several configurations.
Public settings now have hard implementation ceilings. `Limits::validate` and `Checker::new` reject values above them
before scratch allocation; the one-shot `check` API returns a configuration rejection. Optional limits remain bounded by
the hard source and token ceilings when disabled.

The outer source and every nested annotation scan share cumulative byte and token work budgets. An annotation range is
charged again when its contents are interpreted as Python source, so configured work cannot be multiplied by annotation
depth.

The source string is owned by the caller and is not counted as Spam working memory.

## Malformed input

Malformed input follows the same resource limits as valid input.
Lexer errors, mismatched delimiters, unterminated strings, invalid UTF-8 boundaries in token ranges, and incomplete
f-strings must not cause a panic, an unbounded loop, or repeated scanning outside the shared budgets.
All returned byte ranges must be valid boundaries in the input `str`.

## Verification

The safety checks require:

1. A fuzz target that varies both arbitrary UTF-8 source and every public configuration field.
2. Regression cases for maximum permitted stacks and values immediately above each limit.
3. Tests that catch panics and verify returned byte ranges for malformed Unicode input.
4. Scaling tests at increasing input sizes for each generator family.
   Doubling source size should not produce quadratic Spam runtime.
5. Memory and stack constrained runs of the retained corpus on supported platforms.
6. A host deadline around Spam and the downstream parser.
   The deadline remains the final availability limit if an implementation bug violates an expected complexity bound.

Before release, run the source-plus-configuration fuzz target for a sustained campaign.
