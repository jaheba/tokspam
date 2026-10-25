# Configuration and scanning

`Limits` also controls token count, cumulative scan work, indentation depth, scanning quoted annotations, and the
maximum number of nested annotations. Scan work is the sum of bytes admitted to scans and tokens emitted, including
quoted annotations interpreted as Python. Exhausting that budget returns `Reason::Inconclusive` rather than claiming a
specific structural violation.
`check` returns a reason and a byte range for rejected input.
For repeated use, create a `Checker` once per worker.
For fixed `Limits`, scratch-memory ceilings are independent of input length and
structure. `Checker::new(limits)` starts with empty vectors, growing them on demand
within those ceilings. `Checker::preallocated(limits)` reserves the complete bound
immediately; even its first check performs zero heap allocations.
Both modes retain capacity after scans and rejections. Resetting clears lengths,
segment bases, and scalar tracking state, without shrinking buffers. Growing mode
may allocate for a later input that needs additional space. Growth is geometric,
capped explicitly rather than letting `Vec` double beyond its configured ceiling.
The borrowed source itself is not scratch memory. The one-shot `check` creates
and drops a growing checker on each call.
Most ceilings follow configured limits; width tracking uses its hard ceiling because unfinished
lambda parameter lists can accumulate before parser depth is charged. Compact stack entries and shared annotation
segments keep the memory savings independent of the backing storage.
On the measured 64-bit target, `size_of::<Checker>()` includes vector metadata and growth ceilings. Preallocated scratch requests total 25,578 bytes at default
limits and 90,182 bytes at maximum structural and annotation depths, in eight allocations. These figures exclude
allocator overhead and the borrowed source.

```rust
use tokspam::{Checker, Limits};

let mut checker = Checker::new(Limits::default()).expect("valid limits");
checker.check("x = [1, 2, 3]")?;
checker.check("y = x[0]")?;
# Ok::<(), tokspam::Rejection>(())
```

For published package sources, opt into larger byte and token budgets with `Limits::package_sources()`:

```rust
use tokspam::{Checker, Limits};

let mut checker = Checker::new(Limits::package_sources()).expect("valid limits");
checker.check("x = [1, 2, 3]")?;
# Ok::<(), tokspam::Rejection>(())
```

This profile allows 8 MiB source, 500,000 tokens, and 8 MiB + 500,000 cumulative work units.
It preserves the default parser depth (100), AST depth (100), indentation, annotation, and construct-width settings.
`Limits::default()` remains unchanged. The profile is a package-compatibility option, not a validated parser-safety
configuration; hosts must retain deadlines and evaluate their downstream parser and AST pipeline.

`Checker::new` validates configuration and returns `ConfigError` for a value above an implementation
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
| Each optional construct width | 1,000,000 |

The optional AST check counts long binary, attribute, call, and subscript chains that a parser may construct iteratively
but later destroy recursively.
Its depth estimate is conservative. Binary expressions combine the maximum height of their operand subtrees;
postfix chains in separate operands are not added together. Closed groups retain their peak height for later
postfix and binary operators, so grouping cannot erase an already constructed subtree.

A shared structural pass maintains separate parser and AST depth estimates.
A conservative fast path scans a prefix of short, unindented simple lines without constructing tokens.
It counts tokens exactly and uses line byte length as a depth upper bound; complex syntax or insufficient budgets falls
back to detailed scanning.

The lexer handles comments, strings, f-strings, template strings, indentation, and explicit line continuations.
It retains bounded stacks and scans without storing a token list.
Literal-text searches use `memchr`, which selects optimized searches for the target platform. A short scalar prefix
keeps nearby delimiters cheap. Four-delimiter searches use fixed 64-byte blocks to preserve linear behavior when a
fourth delimiter repeats before the other three. These searches add no heap allocation.
Scratch storage is reserved before scanning. Its allocation count and capacities depend only on configuration, and
input cannot grow the buffers, including while scanning nested quoted annotations.
Quoted annotations are scanned with an explicit preallocated work stack; source structure never adds native Rust stack
frames. Suspended scans share segmented scratch buffers, retaining only scalar state per annotation level. Buffer
capacities cover configured structural limits plus annotation depth rather than reserving full buffers for every level.
The one-shot function rejects oversized source before constructing its checker.
String contents are screened only at likely annotation positions, including variable, parameter, and return annotations.
Only the first operand of `Annotated[...]` is treated as a type; subsequent metadata operands and all
`Literal[...]` operands are values, so their strings are not screened as forward references. These exclusions also
apply inside quoted type expressions and recognize qualified names ending in `Annotated` or `Literal`.
This is a lexical heuristic, not a Python grammar check or name resolution; aliases are not resolved and shadowing
of these typing names is not detected. Unknown type constructors retain conservative string screening.
Escaped and concatenated annotations are not decoded.

Unrecognized ASCII code bytes, such as `$`, backticks, and control characters,
produce a `LexicalStructure` rejection. String text and comments bypass this
check. It is not token or syntax validation: Unicode identifiers, numeric spellings,
and f-string conversion grammar still require the downstream parser.

Tokspam exposes independent optional budgets for call arguments, function and lambda parameters, display items, subscript
items, and f-string or template-string interpolation fields.
All five default to `None`, so upgrading does not enable them automatically.
Width budgets are always available; no Cargo feature is required.
The first calibrated profile uses 4,096 for calls, parameters, slices, and interpolation fields and 8,192 for displays.
These values are starting points rather than stable defaults; see the [width-limit
experiment](https://github.com/jaheba/tokspam-fuzzer/blob/main/campaigns/2026-10-24-width-limits/SUMMARY.md).
