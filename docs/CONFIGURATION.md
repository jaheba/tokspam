# Measurement and checking

`analyze(source)` measures with growing storage and no thresholds. For reuse,
construct `Checker::new(Limits::default())` and call `analyze` repeatedly.
`Checker::new` is infallible and does not allocate scratch. Source is borrowed only
for each call; no source or token list is retained.

`Limits` contains optional `usize` thresholds. `None` disables a threshold;
`Some(0)` is a real zero threshold. Values have no arbitrary configuration ceiling.
`Limits::screening()` supplies an explicit pre-parser policy, rather than making
that policy the default. Successful checking is not syntax validation or a parser
safety guarantee.

- `Checker::analyze` collects every score and returns `Report { scores, stopped }`.
- `Checker::check` returns `Result<(), Rejection>`, exiting early and skipping
  unnecessary measurement. The free `check(source, &limits)` is the one-shot form.
- `check_regions`, behind the `regions` feature, enables additional conservative
  statement proofs, with detailed fallback for type strings and uncertain input.

`check_string_annotations` defaults to true. Disabling it excludes quoted-type
rescans from both measurements and checking. Scores still include the outer string
as an ordinary token.

## Storage policy

`Storage::Growing` uses fallible geometric reservation. Allocated memory depends on
observed structure and is retained across checks. Allocation failure stops the scan
with `Reason::Storage`; it is not evidence of a score threshold violation.

`Checker::with_storage(limits, Storage::Fixed { capacity })` reserves `capacity`
entries for each of eight shared stacks, independently of the policy. It returns
`StorageError` if reservation fails. No scanning allocations occur in this mode.
A capacity of zero is valid but cannot start a scan. A capacity of one can scan
simple expressions but cannot retain a nested bracket or annotation frame.

Every call resets logical state, including after failure, while retaining storage.
A fixed checker can therefore continue processing simpler inputs after exhaustion.

## Incomplete analysis

`stopped == None` means the configured measurement completed. Otherwise inspect the
rejection reason and byte range. Partial scores are observed lower bounds, except
that `source_bytes` is always the full input length. No observed total is invented
for unvisited input.

`Inconclusive` denotes the optional cumulative work budget, `Storage` denotes
scratch exhaustion, and `Overflow` denotes unrepresentable accounting. Lexical
failures report `LexicalStructure`. A structural threshold inside a quoted type
is attributed to its enclosing annotation and reported as `QuotedAnnotation`.

Byte ranges refer to the caller's original UTF-8 source, including nested scans.

## Migrating from the screening-only API

Wrap previous numeric thresholds in `Some(...)`, or use `Limits::screening()` for
an explicit starting policy. `Default` now has no thresholds. `Checker::new`
returns the checker directly. Replace `Checker::preallocated` with `with_storage`
and an explicit fixed entry capacity. `ConfigError`, `LimitKind`, and the `MAX_*`
configuration constants have been removed. External integrations must adopt these signatures before rebuilding. The companion
Rust benchmark and fuzz adapters have been migrated to explicit screening policies.
