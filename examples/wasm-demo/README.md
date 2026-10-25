# WebAssembly demo

This page presents Tokspam as a pre-parser resource limit checker and runs it in the browser. Its sidebar has one ordinary module for comparison and five inputs tied to observed CPython or Ruff parser failures and a documented historical Ruff issue. Loading a sample restores default limits but does not run the check. Editing source or changing a limit clears the previous result; **Check limits** performs the check and shows the Wasm scan time. It uses a small C-style WebAssembly export, so no JavaScript package manager or binding generator is required.

`ruff_grouping.py` contains the 5,000-parenthesis input from [Ruff's parser recursion-limit change](https://github.com/astral-sh/ruff/pull/24810). The issue concerned an earlier Ruff parser; current Ruff has a recursion limit for this shape. Tokspam rejects it under the default parser-nesting limit. CPython also rejects this source for excessive nesting.

`ruff_class_pattern.py` is the retained nested class-pattern case from the [generated parser attack campaign](https://github.com/jaheba/tokspam-fuzzer/blob/main/campaigns/2026-10-24-generated-parser-attack/SUMMARY.md). On 2026-10-24, `ruff check --isolated` with Ruff 0.17.2 aborted with a stack overflow on macOS arm64. Tokspam rejected the same source under its default estimated AST-depth limit. This observation is specific to that Ruff version, command, and platform.

The **CPython parser failures** group contains three files that Tokspam rejects before CPython parses them. `cpython_nesting.py` has 220 nested parentheses: CPython 3.15.0 reports `SyntaxError: too many nested parentheses`, while Tokspam hits its default parser-nesting limit. `deep_indentation.py` has 105 nested blocks: CPython 3.15.0 reports `IndentationError: too many levels of indentation`, while Tokspam hits its default indentation limit. `cpython_unary_memory.py` is copied from the [generated parser attack campaign](https://github.com/jaheba/tokspam-fuzzer/blob/main/campaigns/2026-10-24-generated-parser-attack/SUMMARY.md): 5,966 unary minus operators in 5,972 bytes produced `MemoryError` under the campaign's CPython 3.15.0 conditions; Tokspam rejects the source under its default parser-nesting limit. This is a memory failure, not a measured time explosion. Outcomes and boundaries can vary by Python version and environment.

The primary controls expose parser nesting, estimated AST depth, and quoted annotation depth. Source bytes (entered as KiB), token count, and indentation are in the expandable **Other limits** section. The demo uses the explicit `Limits::screening()` preset. Control ranges reflect the wasm32 ABI rather than library score ceilings. The cumulative work budget follows source bytes plus tokens. Other optional library limits keep their defaults.
See [what each limit does](../../docs/LIMITS.md) for detailed explanations and the limits not exposed in this demo.

The editor uses a native textarea with a lightweight syntax-colored display and line-number gutter. Highlighting pauses for very large source so editing remains usable; screening uses the source-size threshold selected in the controls.
The result panel names the failed validation, shows the threshold used for that run when one applies, and maps the returned UTF-8 byte range to a line and column. **Show in editor** selects that range. Tokspam does not return an observed depth or token total, so the panel does not invent one.

```sh
rustup target add wasm32-unknown-unknown
sh examples/wasm-demo/build.sh
python3 -m http.server 8000 --directory examples/wasm-demo
```

Open <http://localhost:8000>. A local HTTP server is needed because browsers generally do not allow `fetch` from a `file://` page. The build output is ignored by Git. The demo does not parse or run Python source; it shows Tokspam's screening result and UTF-8 byte range. A single browser timing is illustrative rather than a performance benchmark.

## GitHub Pages

The [Pages workflow](../../.github/workflows/wasm-demo-pages.yml) builds the Wasm module and publishes only the HTML, JavaScript, and Wasm files when relevant changes reach `main`. In the repository's **Settings → Pages → Build and deployment**, set **Source** to **GitHub Actions**. The expected project URL is <https://jaheba.github.io/tokspam/> after the first successful deployment.

The demo uses checking; library callers can collect full `Scores` with `analyze`.
The Wasm bridge returns the reason separately from a packed pair of full-width
wasm32 byte offsets, avoiding the previous source-size-dependent range encoding.
