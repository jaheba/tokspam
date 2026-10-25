# What each limit does

Tokspam screens source **before** a Python parser sees it. Its limits are budgets for source volume and structural
complexity, not rules about what the program may do. A rejection means a budget was exceeded; acceptance does not
prove that the source is valid Python or that a downstream parser will process it safely. Keep a host deadline and
resource controls around parsing. See [safety requirements](SAFETY.md) for the implemented bounds and remaining
validation work.

The [WebAssembly demo](../examples/wasm-demo/README.md) exposes six of the controls below: source size, token count,
parser nesting, indentation depth, estimated AST depth, and quoted annotation depth. It uses the library defaults
unless you change them. The remaining settings are available through the Rust `Limits` struct.
The code snippets below are small illustrations of each shape; a larger version or a tighter configured limit is
needed to trigger most rejections.

## Parser and AST protection

### Parser nesting — `max_nesting_depth` (default: 100)

This caps Tokspam's estimate of simultaneously retained parser frames. Open brackets, nested calls, prefix operators,
conditional expressions, and some f-string constructs can add to it. The concern is a parser that recurses deeply or
allocates many live frames before it can finish an expression. The demo's CPython parenthesis and unary-operator cases
hit this limit. It is an estimate of parser pressure, not a measurement of CPython's call stack.

```python
result = f(g(h(value)))  # More nested calls retain more parser frames.
```

### Estimated AST depth — `max_ast_depth` (default: 100)

Some source has shallow parser nesting but creates a deep tree. Long attribute, call, subscript, and operator chains
can wrap earlier expressions repeatedly; flat `elif` chains also become nested `If` nodes in Python's AST. A later
AST traversal or destruction may then need substantial stack space. Tokspam tracks an estimated tree height and
rejects above this budget. The estimate is deliberately structural, not an exact AST build. `None` disables this
separate check.

```python
result = value.a.b.c.d  # Keep extending the attribute chain.
```

### Quoted annotation depth — `max_annotation_depth` (default: 8)

A type annotation such as `item: "list[int]"` is one string token in the outer source. A downstream tool may later
parse the string's contents as a Python type expression, so Tokspam may start a second scan over `list[int]`. If that
expression contains another quoted type, it may start another scan over that inner string. The limit counts these
successive scans, **not** the number of `list[...]` brackets. Short strings can be cleared without a rescan when
their byte length proves they cannot exceed the remaining depth budget. All scans share parser-depth, token, byte,
and work budgets; the nesting cap prevents repeated quoted layers from multiplying screening work without bound.

```python
item: """list['''tuple["list['int']"]''']"""
# Potential scan layers: file → list[...] → tuple[...] → list[...] → int.
```

`check_string_annotations` defaults to `true`. Setting it to `false` disables those inner scans; it is a switch rather
than a numeric limit. Type positions are recognized lexically, without name resolution or decoding escaped strings.
See [configuration](CONFIGURATION.md) for the exact scope.

## Supporting limits

### Source size — `max_source_bytes` (default: 1 MiB)

This caps the UTF-8 byte length of the entire input, including whitespace, comments, and string literals. A file can be
small in line count yet contain a very large literal or generated expression. Tokspam rejects oversized input before
allocating its scan scratch. This limits how much source it must inspect; it does not bound memory already used by the
caller to hold the source.

```python
data = "a very long literal ..."  # The literal text itself counts toward file bytes.
```

### Token count — `max_tokens` (default: 100,000)

This caps the tokens emitted by the streaming lexer. A source file can fit the byte budget while containing very many
short statements or operators. Token count limits the amount of token-driven scan work and helps cap work presented to
a downstream parser. It counts tokens across nested annotation scans too; it is not a line or AST-node count.

```python
a = 1; b = 2; c = 3  # Repeating short statements adds many tokens.
```

### Indentation depth — `max_indentation_depth` (default: 80)

This caps active indentation levels. One hundred nested `if` blocks differ from one hundred independent top-level
`if` statements: only the nested version keeps many blocks open at once. The limit catches deeply nested suites, such
as the demo case that CPython 3.15.0 reports as `too many levels of indentation`. It does not count total lines or
the number of spaces on a single line.

```python
if a:
    if b:
        pass  # Keep nesting blocks to increase active indentation depth.
```

### Cumulative scan work — `max_work_units` (default: 1 MiB + 100,000)

Tokspam charges bytes admitted to scans plus tokens emitted. Quoted annotations may be scanned again as Python source,
so their contents consume this shared budget again. This prevents repeated nested scans from multiplying work without
limit. If the budget runs out before a structural violation is established, the result is `Inconclusive`, not a claim
that the source exceeded a particular parser limit. The demo adjusts this budget to match its source-size and token
controls; it has no separate work-budget input.

```python
item: "'list[int]'"  # Reinterpreting quoted annotation contents charges scan work again.
```

### Single-token size — `max_token_bytes` (default: disabled)

This optional cap rejects one emitted token whose UTF-8 range is too long. It is useful when the downstream parser or
other tooling has costly handling for an exceptionally long name or literal. A byte limit on the whole file alone does
not distinguish one enormous token from many ordinary ones. `None` disables this separate rule; the source-size and
work budgets still apply.

```python
an_exceptionally_long_identifier_name = 1  # One name is one token.
```

## Optional construct-width limits

Depth limits do not cap a very wide, flat construct. These five limits default to `None`, so the separate width rule is
off. Source-size, token-count, and work budgets still apply. Configure them for the parser and workload you actually
use; the [width-limit experiment](https://github.com/jaheba/tokspam-fuzzer/blob/main/campaigns/2026-10-24-width-limits/SUMMARY.md)
records an initial calibration, not universal safe values.

| Setting | Example shape | Why it can matter |
| --- | --- | --- |
| `max_call_arguments` | `f(a, b, c)` | A huge call can make parser and AST work large without deep nesting. |
| `max_parameters` | `def f(a, b, c): pass` | A very wide signature can burden parsing and later analysis. |
| `max_display_items` | `[a, b, c]` | A wide literal can create a large AST while staying shallow. |
| `max_slice_items` | `value[i, j, k]` | A wide subscript can create substantial parser and AST work. |
| `max_interpolation_fields` | `f"{a}{b}{c}"` | A large number of fields can burden parsing and formatting-related analysis. |

## Ruff parser examples

These examples show where screening can stop source **before** a parser encounters a troublesome shape. They are
version-specific evidence, not proof that Tokspam catches every Ruff failure.

### Deep grouping: historical parser stack overflow

Ruff's [parser recursion-limit change](https://github.com/astral-sh/ruff/pull/24810) names 5,000 pairs of parentheses
around `1` as a source shape that could previously overflow its parser stack. The change added a recursion limit in
May 2026. Tokspam rejects that shape under its default parser-nesting limit:

```rust
use tokspam::{Limits, Reason, check};

let source = format!("{}1{}", "(".repeat(5_000), ")".repeat(5_000));
let rejection = check(&source, &Limits::default()).unwrap_err();
assert_eq!(rejection.reason, Reason::Nesting);
```

The demo's `ruff_grouping.py` contains the same grouping pattern. This illustrates a historical Ruff stack-overflow
case; it does not reproduce a crash in Ruff versions with the recursion limit.

### Nested class patterns: observed Ruff process crash

The [generated parser attack campaign](https://github.com/jaheba/tokspam-fuzzer/blob/main/campaigns/2026-10-24-generated-parser-attack/SUMMARY.md)
recorded a process crash in its tested Ruff build while dropping a deeply nested class-pattern tree. Its retained
case has 86,959 nested `C(...)` patterns. A fresh `ruff check --isolated` run with Ruff 0.17.2 on macOS arm64 also
aborted with `stack overflow` on the exact retained file. Tokspam rejects the same source under its default estimated
AST-depth limit:

```rust
use tokspam::{Limits, Reason, check};

let source = format!(
    "match x:\n    case {}0{}: pass\n",
    "C(".repeat(86_959),
    ")".repeat(86_959),
);
let rejection = check(&source, &Limits::default()).unwrap_err();
assert_eq!(rejection.reason, Reason::AstDepth);
```

The crash was observed with this source, command, platform, and version; it should be retested when Ruff changes. The
example demonstrates the value of limiting tree depth before handing source to a parser and its later AST cleanup
paths.

## Choosing values

Start with the defaults, measure the real sources your application accepts, then tighten or raise limits deliberately.
The Rust library enforces hard configuration ceilings listed in [configuration](CONFIGURATION.md); the
`Limits::package_sources()` profile raises only the source, token, and work budgets for larger package files. A limit
that is safe for one parser version, platform, or workload is not a general parser-safety guarantee.
