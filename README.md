# Tokspam

Tokspam measures structural complexity in Python source without building an AST.
Use it to inspect generated code, compare source shapes, or apply optional limits
before sending input to a parser.

A short expression can still create a deep tree or retain many parser frames:
nested containers, unary operators, attribute chains, and quoted type expressions
all put pressure on different parts of a parser. Source size alone misses these
shapes. Tokspam reports counts and depth estimates that make them visible.

```rust
use tokspam::analyze;

let report = analyze("result = a + b + c");
assert!(report.is_complete());
assert_eq!(report.scores.ast_depth, 2);
```

## What it measures

The streaming lexer emits source ranges, without decoding literals or collecting
all tokens. Structural trackers maintain open levels and operator state. They
estimate parser recursion and AST depth, preserving the maximum across sibling
expressions rather than adding their independent heights.

Reports contain source bytes, cumulative scanned bytes and tokens, largest token,
lexical context depth, indentation depth, estimated parser and AST depths,
quoted-annotation depth, and maximum argument, parameter, display, slice, and
interpolation-field counts. Lexical context depth includes string and replacement
field modes as well as brackets.

Quoted types are scanned using an explicit work stack. A child scan inherits its
parent's remaining structural budget, and its tokens and bytes contribute to the
same cumulative work count. `Literal` values and `Annotated` metadata are excluded
using lexical names. Escaped plain annotation strings are decoded into reusable
scratch before scanning their values; ordinary literals remain source slices.
Tokspam does not resolve aliases. Named Unicode escapes (`\N{...}`) and isolated
surrogates stop annotation measurement explicitly with `AnnotationEscape`.

These scores describe Tokspam's lexical model. Tokspam does not validate full
Python syntax, execute source, or guarantee that a particular parser will handle
accepted input safely. A malformed lexical structure can stop measurement.

## Optional limits

Every score threshold is optional. `Limits::default()` enables no thresholds;
there are no library-imposed configuration ceilings. Use a reusable `Checker`
when applying a policy to many inputs:

```rust
use tokspam::{Checker, Limits};

let mut checker = Checker::new(Limits {
    max_ast_depth: Some(80),
    max_call_arguments: Some(200),
    ..Limits::default()
});

checker.check("f(1, 2)").unwrap();
let report = checker.analyze("a + b + c");
assert!(report.is_complete());
assert_eq!(report.scores.ast_depth, 2);
```

`check` stops at the first exceeded threshold and omits unused score collection.
It retains conservative shortcuts for simple input. `analyze` performs detailed
measurement, stopping at configured thresholds if supplied. `Limits::screening()`
is an explicit starting policy with thresholds; tune it for your downstream parser.

## Memory and incomplete reports

Growing storage is the default. Scratch grows fallibly with structural complexity
and is reused between calls. Removing score limits does **not** imply constant
memory: measuring arbitrary nesting needs storage for the open levels.

For allocation-free scans, select fixed storage explicitly:

```rust
use tokspam::{Checker, Limits, Storage};

let mut checker = Checker::with_storage(
    Limits::default(),
    Storage::Fixed { capacity: 256 },
).unwrap();

let report = checker.analyze("x = [1, 2]");
assert!(report.is_complete());
```

The capacity is an entry count **per scratch stack**, plus a byte capacity for
decoded annotation text, independent of score limits.
It is reserved during construction; scans cannot allocate additional scratch.
Exhausting it yields `Reason::Storage`, not a complexity violation.

Always inspect `report.is_complete()` or `report.stopped`. A threshold, work-budget
exhaustion, storage failure, unsupported lexical structure, or accounting overflow
can stop a scan. Partial maxima are lower bounds on the model's completed scores;
`source_bytes` always describes the entire input.

## Validation and details

Tests cover structural rules, malformed and Unicode input, quoted annotations,
score boundaries, buffer reuse, and allocation behavior. Previous parser and
library campaigns live in [tokspam-fuzzer](https://github.com/jaheba/tokspam-fuzzer);
their results apply to the tested revisions, versions, and conditions.

See [configuration](docs/CONFIGURATION.md), [score definitions](docs/LIMITS.md),
[implementation](docs/INTERNALS.md), and [safety requirements](docs/SAFETY.md).
The [browser demo](https://jaheba.github.io/tokspam/) demonstrates limit checking.
