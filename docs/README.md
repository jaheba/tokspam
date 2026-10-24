# Tokspam documentation

- [Configuration and scanning](CONFIGURATION.md): limits, reusable checkers, and scanning behavior.
- [Safety requirements](SAFETY.md): resource bounds, invariants, and release criteria.

See the root [README](../README.md) for an introduction. Fuzzing, corpus generation,
benchmarks, and campaign reports live in the separate `tokspam-fuzzer` project.

Run library checks from the repository root:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```
