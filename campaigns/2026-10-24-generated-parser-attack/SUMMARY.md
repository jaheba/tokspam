# Generated parser attack campaign

Python 3.15.0 and Ruff 0.0.14 parsed 851 deterministic grammar cases on macOS with a one-second subprocess deadline.
There were 89 parser failures and no case where a failing parser was reached after Spam accepted the source.
The run also recorded 363 conservative Spam rejections of sources accepted by both parsers.

The failures included CPython `MemoryError` and `RecursionError`, Ruff process crashes while dropping class, mapping,
and postfix pattern trees, and timing-sensitive Ruff format-spec timeouts.
Fifty new shortest family/outcome shapes and twelve known cases were added to SQLite run 31.

The stable generated-depth boundaries retained under `minimized/` are:

| Implementation | Family | Outcome | First reproduced depth | Bytes |
| --- | --- | --- | ---: | ---: |
| CPython | unary | `MemoryError` | 5,966 | 5,972 |
| CPython | f-string attribute chain | `RecursionError` | 95,080 | 190,171 |
| Ruff | nested class pattern | process crash | 86,959 | 260,903 |

The Ruff format-spec timeout was not monotonic in adjacent controls and is explicitly marked unstable in
`minimized/summary.json`.
