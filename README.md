# tokspam

Tokspam is a Rust library that screens Python source before it reaches a parser.
It provides a first layer of resource checks for applications that accept
untrusted or generated Python code. It is designed for fast preflight screening,
with a scratch-memory ceiling determined by its configured limits.

## Why check before parsing?

Parsing untrusted code is itself a resource-sensitive operation. Some source
shapes can exhaust a parser's call stack or produce an AST that is too deep to
traverse or free safely. This can happen with valid Python, before any of it
executes. Malformed input can also stress recursive error-recovery paths before
the parser returns a syntax error.

A bracket-depth limit catches obvious nesting, but misses other shapes:

```python
f(g(h(value)))       # Nested calls can retain parser frames.
value.attr.attr.attr # Flat-looking source can build a deep AST.
a + b + c + d        # Left-associated operators also build a nested tree.
```

At large enough scales, these can turn a parse request into a process failure or
excessive resource use. A runtime instruction budget cannot help if parsing fails
before execution starts.

Tokspam addresses this with a bounded structural scan before the parser runs.
It rejects inputs whose estimated complexity exceeds your budgets, without
constructing the AST that might cause the problem. This adds a first layer of
protection alongside the parser's own checks and the host's resource controls.

## Who is it for?

Tokspam is for Rust applications that embed a Python interpreter, offer a Python
playground, or parse user-submitted code for analysis. Place it before each parser
entry point, including source generated for `eval()` or `exec()`.

It lets you reject inputs that exceed your resource budgets before paying the
cost of parsing them. Limits cover source size, tokens, nesting, indentation,
estimated AST depth, and cumulative scanning work.

See [what each limit does](docs/LIMITS.md) for the rationale and tradeoffs behind every setting.

## Efficiency

Tokspam uses a streaming, byte-oriented lexer and compact structural accounting.
It builds neither an AST nor a token list, uses optimized byte searches to skip
literal text, and stops as soon as a budget is exceeded. Ordinary scanning is
linear in the bytes and tokens processed; annotation rescans share a bounded work
budget.

**The scratch-memory ceiling depends on `Limits`, not input length.** By default,
buffers start empty and grow on demand, up to that ceiling. For a fixed allocated
footprint and allocation-free checks, opt into `Checker::preallocated(limits)`.
The source is borrowed, so its storage remains the caller's responsibility.

Both modes retain buffer capacity between checks. Growing mode may allocate when
a new input needs more space; preallocated mode reserves everything up front and
never allocates while scanning.

## Usage

The crate is named `tokspam`. Create a reusable checker with limits suited to your
application, then pass accepted source to your parser:

```rust
use tokspam::{Checker, Limits};

fn main() -> Result<(), tokspam::Rejection> {
    let limits = Limits {
        max_source_bytes: 256 * 1024,
        max_nesting_depth: 80,
        max_ast_depth: Some(80),
        ..Limits::default()
    };
    let mut checker = Checker::new(limits).expect("valid limits");
    let source = "x = [1, 2, 3]";

    checker.check(source)?;
    // Pass source to your Python parser here.
    Ok(())
}
```

A rejection includes a reason and a byte range. Reuse one `Checker` per worker.
`Checker::new(limits)` starts with empty vectors. Checks grow them only as needed,
within configured bounds, and retain that capacity for later inputs. Oversized
inputs are rejected before initializing scratch storage.

To avoid allocations during checks, opt into full preallocation:

```rust
let mut checker = tokspam::Checker::preallocated(tokspam::Limits::default())
    .expect("valid limits");
checker.check("x = 1").expect("source within limits");
```

Each scan clears the buffers' logical contents and tracking state, including after
a previous rejection. It retains their capacity without shrinking. Default buffers
can grow for more demanding inputs; preallocated buffers never need to grow.
The one-off `tokspam::check` creates and drops a lazy checker on each call.

For larger package files, `Limits::package_sources()` increases the source and
token budgets while keeping the default structural limits. See
[configuration](docs/CONFIGURATION.md) for details.

## How it detects problematic source

Tokspam streams tokens into a structural scanner, maintaining separate estimates
for parser recursion and AST depth. It does not need a complete parse to identify
these resource patterns.

### Estimate retained parser frames

The scanner tracks open delimiters and indentation, plus constructs that can
retain recursive expression frames: prefix and right-associative operators,
lambda and conditional bodies, and nested f-string format fields. It releases
those charges at the boundaries that finish the relevant expression or block.

This is about simultaneous depth, not total syntax. In `f(a, b, c)`, the arguments
are siblings; in `f(g(h(x)))`, their nesting accumulates. An oversized input can
be rejected as soon as the retained-depth estimate exceeds the configured limit.

### Account for deep ASTs even when parsing is shallow

Closing a parser frame does not necessarily remove its contribution to the AST.
Each attribute access, call, or subscript can wrap the preceding expression.
Tokspam retains that contribution when estimating tree depth.

For binary operators it tracks precedence and operand heights. Combining two
subtrees contributes the larger child height plus the operator node; sibling
heights are not simply added together. Expression boundaries reset local state,
while a completed nested expression carries its peak height back to its parent.
Flat `elif` chains also need accounting because Python represents them as nested
`If` nodes.

This catches long chains without treating every operator in a file as part of
one growing expression. It remains a structural estimate rather than an exact
AST measurement.

### Screen source hidden in quoted annotations

Downstream tools may interpret a quoted annotation such as
`item: "list[list[int]]"` as another Python expression. Tokspam recognizes likely
type positions and screens relevant string contents using the remaining depth
budget. It distinguishes type operands from `Annotated` metadata and `Literal`
values using lexical names; it does not resolve aliases or decode escapes.

Nested annotation scans share byte and token budgets. Reinterpreting string
contents charges work again, preventing nested strings from multiplying the
permitted scanning work unchecked.

### Bound width, total work, and the checker itself

Depth limits alone cannot bound a large flat input. Tokspam also limits source
bytes, tokens, and cumulative scan work, with optional width limits for arguments,
parameters, displays, subscripts, and interpolation fields. Exhausting the scan
work budget returns `Inconclusive`, rather than asserting a specific structural
violation.

The scan builds neither an AST nor a token list. Simple unindented lines can use
a conservative byte-based fast path; other input uses detailed structural
accounting. Bounded reusable buffers cap scratch memory, and an explicit annotation
work stack keeps native call-stack depth independent of source nesting.

## Testing and compatibility

Tokspam is tested against real-world Python libraries as well as deliberately
hostile input. Recorded whole-library sweeps using `Limits::package_sources()`
accepted all **2,437 Python files from PyTorch 2.14.1** and **2,057 files from
CPython 3.15.1's `Lib/`, including its tests**. Three intentionally invalid
encoding fixtures were excluded before screening. An earlier calibration also
accepted all 2,955 retained files from CPython 3.15, Monty, and Pydantic.

Validation combines:

- **Focused regressions** for limit boundaries, operator precedence, Unicode,
  malformed source, annotations, and checker reuse after rejection. Allocation
  tests verify bounded growth, capacity reuse, and allocation-free preallocated checks.
- **Generated parser comparisons** against CPython and Ruff, with retained
  reproducers for crashes, resource failures, and conservative rejections.
- **Over six million fuzz executions** across raw-source/configuration and
  grammar-guided targets under AddressSanitizer, with no crash or sanitizer
  finding in the recorded extended campaign.

These are source-screening results, not claims about importing or running those
libraries. The [library compatibility report](https://github.com/jaheba/tokspam-fuzzer/blob/main/corpus/library-suites/README.md),
[calibration results](https://github.com/jaheba/tokspam-fuzzer/blob/main/campaigns/2026-10-24-calibration/SUMMARY.md),
and [fuzzing report](https://github.com/jaheba/tokspam-fuzzer/blob/main/campaigns/2026-10-24-shared-scratch-long-fuzz/SUMMARY.md)
record the tested versions, profiles, inputs, and conditions.

## Scope

Tokspam complements your parser's limits and your host's time and memory controls.
It does not validate Python syntax or restrict what a program can do. Acceptance
does not guarantee downstream parser safety, and conservative checks can reject
valid Python.

See [configuration](docs/CONFIGURATION.md) for available limits and
[safety requirements](docs/SAFETY.md) for the implemented bounds and their limits.
Corpus generators, parser comparisons, fuzzing, and benchmarks live in
[`tokspam-fuzzer`](https://github.com/jaheba/tokspam-fuzzer).

Licensed under [MIT](LICENSE).
