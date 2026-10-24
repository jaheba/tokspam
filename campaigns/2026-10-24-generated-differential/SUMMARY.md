# Generated width differential

The deterministic mixed-grammar run checked 1,000 CPython-valid programs.
For every program it measured call, parameter, display, slice, and interpolation widths from the CPython AST, then
tested Spam at the exact boundary and one below.
Ruff parsed every source.
There were zero width mismatches and zero parser failures; no cases were retained in SQLite run 29.

A second 100-case run used Pyrefly 1.4.0 as an additional downstream checker.
It also produced zero width mismatches, Ruff failures, or Pyrefly process failures (SQLite run 30).
