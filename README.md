# tokspam

Tokspam is a Rust library that screens Python source before it reaches a parser.
It provides a first layer of resource checks for applications that accept
untrusted or generated Python code.

Deep nesting is only part of the problem: long operator chains, repeated calls,
and quoted type annotations can also create excessive parser or AST depth.
Tokspam detects these source shapes without building an AST or executing Python.

## Who is it for?

Tokspam is for Rust applications that embed a Python interpreter, offer a Python
playground, or parse user-submitted code for analysis. Place it before each parser
entry point, including source generated for `eval()` or `exec()`.

It lets you reject inputs that exceed your resource budgets before paying the
cost of parsing them. Limits cover source size, tokens, nesting, indentation,
estimated AST depth, and cumulative scanning work.

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

A rejection includes a reason and a byte range. Reuse one `Checker` per worker:
construction and scanning perform no heap allocations. Scratch storage uses
fixed-capacity inline arrays sized for the hard limits; smaller configured limits
do not shrink the checker. For a one-off check, use `tokspam::check`.
The library has no runtime dependencies.

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
