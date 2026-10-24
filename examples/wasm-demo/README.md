# WebAssembly demo

This page runs Tokspam in the browser with its default limits. The sample buttons load an ordinary Python module that passes and a deeply nested expression that triggers a nesting rejection. It shows the Wasm scan time for each run. It uses a small C-style WebAssembly export, so no JavaScript package manager or binding generator is required.

```sh
rustup target add wasm32-unknown-unknown
sh examples/wasm-demo/build.sh
python3 -m http.server 8000 --directory examples/wasm-demo
```

Open <http://localhost:8000>. A local HTTP server is needed because browsers generally do not allow `fetch` from a `file://` page. The build output is ignored by Git. The demo does not parse or run Python source; it shows Tokspam's screening result and UTF-8 byte range. A single browser timing is illustrative rather than a performance benchmark.

## GitHub Pages

The [Pages workflow](../../.github/workflows/wasm-demo-pages.yml) builds the Wasm module and publishes only the HTML, JavaScript, and Wasm files when relevant changes reach `main`. In the repository's **Settings → Pages → Build and deployment**, set **Source** to **GitHub Actions**. The expected project URL is <https://jaheba.github.io/tokspam/> after the first successful deployment.
