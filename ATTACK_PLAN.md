# Parser attack plan

Spam protects the availability of the tokenizer, parser, AST pipeline, and compiler stages that consume screened source.
It does not judge what a Python program will do when executed.

An attack succeeds when Spam accepts the source and a downstream stage exceeds its time or memory budget, exhausts the
native stack, panics, aborts, hangs, or corrupts its result.
A controlled syntax or nesting error is not a failure.

## Campaign budgets

Routine campaigns run every implementation in a disposable process and measure each phase separately.
The initial budgets are:

| Budget | Candidate threshold | Hard campaign limit |
| --- | ---: | ---: |
| Parse and AST disposal | 100 ms | 250 ms |
| Incremental peak resident memory | 64 MiB | 256 MiB process ceiling |
| Source size | 1 MiB | 1 MiB |

The 100 ms threshold is an admission target, not a portable benchmark assertion.
Each run records parser version, build profile, platform, native stack size, source size, elapsed time, and peak
resident memory.
Scaling runs compare successive input sizes on the same machine; superlinear growth is a finding even before an absolute
threshold is crossed.

A one-second timeout may be used while discovering a threshold.
Five seconds is reserved for confirming or minimizing an already isolated failure and is never the normal admission
budget.

Before fixing these numbers as defaults, measure valid standard-library and representative generated files on supported
platforms.
Choose a production budget above the high percentile of valid inputs while retaining a substantial margin below the
worker deadline.

## Outcomes to record

The database records these outcomes independently for tokenization, parsing, AST construction, AST traversal, AST
destruction, and compilation:

- success;
- controlled syntax or nesting rejection;
- Spam rejection;
- soft time or memory threshold exceeded;
- hard timeout or memory limit;
- `RecursionError` or `MemoryError`;
- panic, assertion, signal, or other abnormal exit;
- invalid AST or inconsistent parser result.

Keep the shortest reproducer for each implementation, version, phase, and outcome.
Retain negative results as campaign metadata so later work explores different constructions.

## Native stack exhaustion

Attack every phase that may walk source or AST structure recursively.

Generate:

- nested brackets, lambdas, conditional expressions, powers, patterns, targets, annotations, and format specifications;
- flat binary, attribute, call, subscript, slice, comparison, Boolean, assignment, and `elif` chains;
- pairwise and three-way mixtures of those forms;
- valid deep structures followed by one late syntax error;
- the same expression inside functions, classes, comprehensions, f-strings, annotations, patterns, and decorators.

Run each family with a small native stack and use bounded threshold search to find the first failing size.
Instrument Ruff before and after parse and before and after AST disposal so a destructor overflow is not attributed to
parsing.
For CPython, probe `ast.parse`, AST traversal, `compile`, and object disposal separately.

Existing retained cases cover deep power and lambda expressions, long binary and postfix chains, slices, mixed postfix
chains, nested class bodies, a flat `elif` chain, malformed mixed nesting, and recursive Ruff error recovery.

## Excessive CPU time

Look for superlinear work and long flat work that does not require structural depth.

Generate increasing sizes for:

- many f-string replacement fields and nested format specifications;
- long valid prefixes with an error in the final token;
- repeated ambiguous soft keywords and pattern constructs;
- comprehensions with many filters and generators;
- adjacent string literals and escape-heavy strings;
- Unicode identifiers requiring normalization;
- decimal, hexadecimal, floating-point, imaginary, and malformed numeric literals;
- errors that require constructing a diagnostic near the beginning of a long line.

Measure at geometrically increasing sizes and fit the elapsed-time ratio.
Flag a family when doubling the input repeatedly costs materially more than twice as much, or when an accepted source
crosses 100 ms.
The known flat-f-string case belongs here: CPython took seconds while lexical nesting remained shallow.

## Excessive memory

Measure peak resident memory above an empty-process and tiny-source baseline.
Exercise sources that can amplify into many nodes, parser states, decoded bytes, or temporary buffers:

- flat literals, calls, parameters, patterns, imports, aliases, decorators, and exception handlers;
- long chains that create one AST node per short source fragment;
- escape-heavy strings and adjacent strings;
- large integers and floats;
- malformed inputs that may retain recovery states or diagnostics;
- combinations that cause both a concrete syntax tree and AST to remain live.

Search for high peak-memory-to-source-size ratios as well as absolute failures.
Flag Spam-accepted input above 64 MiB of incremental peak memory.
Run probes under a 256 MiB process ceiling so allocator failure cannot affect the campaign host.

## Malformed-input recovery

Start from every valid generator family and place an error at the beginning, middle, and final token.
Mutations include deletion, duplication, substitution, mismatched closers, missing colons, unterminated strings, invalid
prefixes, escaped newlines, CRLF boundaries, comments, NUL bytes, and invalid operator or keyword sequences.

Prefer mutations that leave a long valid prefix before failure.
Combine errors with deep and flat structures rather than testing only one malformed token.
Compare early and late failure to detect recovery paths whose work depends on all preceding parser state.

The retained corpus already includes truncation and delimiter, quote, colon, and line-continuation mutations.
Repeated invalid `if` tokens previously overflowed Ruff's stack and are now blocked by the token budget.

## Lexer disagreement

Differentially generate each transition in Spam's lexer and compare its structural accounting with CPython and Ruff.
Concentrate on:

- every valid and invalid string-prefix combination;
- single, double, and triple quotes;
- raw, bytes, formatted, and template strings;
- braces, comments, conversions, debug expressions, and nested format specifications in f-strings;
- quote reuse and newlines permitted by current Python f-string grammar;
- explicit continuations, CRLF, form feeds, tabs, and comments at indentation boundaries;
- Unicode identifiers and malformed tokens adjacent to non-ASCII characters;
- mismatched delimiters inside and around string interpolation.

A disagreement is dangerous when Spam treats parser-visible structure as string text, a comment, or one harmless token.
Minimize and retain disagreements even when the parser does not yet exceed a resource budget; they are ingredients for
mixed attacks.

## Flat syntax with deep output

Use CPython AST inspection to measure actual output depth and node count for sources whose lexical nesting is shallow.
Generate and combine `elif`, postfix operations, operators with every associativity and precedence, conditional
expressions, pattern `as` and `|`, comprehension clauses, decorators, assignment targets, exception handlers, type
parameters, and new grammar added by each supported Python release.

For each family, compare Spam's estimated depth with the constructed AST depth.
A harmful underestimate becomes a regression case.
A large overestimate becomes a false-positive case.

## Composition attacks

Single-family generators do not expose interactions between ordinary parser frames.
Build a grammar matrix that combines two or three independently bounded dimensions, including:

- bracket depth with prefix operators or powers;
- class, function, and indentation depth with expression depth;
- f-string depth with format-specification and expression depth;
- lambdas with conditional expressions and comprehensions;
- patterns with guards and type expressions;
- decorators, parameters, annotations, and default expressions;
- malformed recovery inside each mixture.

Use covering arrays before random generation so every pair and selected triples are exercised at boundary values.
The minimized CPython parser-stack failure demonstrates this class: several ordinary grammar frames accumulated into a
failure without one exotic token.

## Stage-specific failures

One source is run through independent probes for:

1. tokenization;
2. parse and AST construction;
3. a full iterative AST traversal;
4. AST destruction;
5. bytecode compilation;
6. cleanup after a controlled syntax error.

Each probe reports immediately before and after its phase.
If a process terminates, the last marker identifies the failing stage.
Do not execute generated programs in the parser campaign.

## Parser entry points and version drift

Screen source immediately before every parser entry point, including initial compilation and runtime-generated source.
Spam accepts calls such as `eval`; an integration must screen the string passed to the later parser invocation.

Replay retained cases against every supported CPython and Ruff version, debug and release builds, supported platforms,
and at least two native stack sizes.
A controlled rejection in one version does not establish safety in another.
Record binary hashes and complete run settings with every observation.

## Order of work

1. Split the Ruff and CPython probes into parse, traverse, drop, compile, and recovery phases.
2. Record elapsed time and peak resident memory for every phase.
3. Lower the routine timeout to 250 ms and add the 100 ms soft finding threshold.
4. Add systematic mixed-family and late-error generators.
5. Add lexer-transition differential generation, beginning with f-strings.
6. Calibrate time and memory thresholds against the standard library and representative accepted programs.
7. Replay and minimize every accepted failure, then add it to the database before changing Spam's rules.
