# Tokspam documentation

- [Configuration and scanning](CONFIGURATION.md): limits, reusable checkers, and scanning behavior.
- [What each limit does](LIMITS.md): what each budget measures and the parser pressure it addresses.
- [How the implementation fits together](INTERNALS.md): scan flow, shared state, and fast paths.
- [Safety requirements](SAFETY.md): resource bounds, invariants, and release criteria.

See the root [README](../README.md) for an introduction. Fuzzing, corpus generation,
benchmarks, and campaign reports live in the separate `tokspam-fuzzer` project.

Run library checks from the repository root:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```
