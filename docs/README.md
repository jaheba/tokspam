# Tokspam documentation

- [Measurement and checking](CONFIGURATION.md): optional thresholds, reports, reuse, and storage.
- [Score definitions](LIMITS.md): counts, depth estimates, widths, and corresponding thresholds.
- [How the implementation fits together](INTERNALS.md): scan flow, shared state, and fast paths.
- [Safety requirements](SAFETY.md): resource bounds, invariants, and release criteria.

See the root [README](../README.md) for an introduction. Fuzzing, corpus generation,
benchmarks, and campaign reports live in the separate `tokspam-fuzzer` project.

Run library checks from the repository root:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```
