const reasonNames = [
  "Pass", "Source size", "Token count", "Token size", "Nesting", "Indentation",
  "AST depth", "Lexical structure", "Quoted annotation", "Configuration",
  "Call arguments", "Parameters", "Display items", "Slice items",
  "Interpolation fields", "Inconclusive", "Reserved", "Invalid UTF-8",
];

const source = document.querySelector("#source");
const result = document.querySelector("#result");
const checkButton = document.querySelector("#check");
const normalButton = document.querySelector("#normal");
const nestedButton = document.querySelector("#nested");
const encoder = new TextEncoder();
let wasm;

function screen() {
  const bytes = encoder.encode(source.value);
  if (bytes.length > 16 * 1024 * 1024) {
    result.dataset.state = "reject";
    result.textContent = `Source size: ${bytes.length} bytes exceeds the hard 16 MiB ceiling.`;
    return;
  }

  const ptr = wasm.demo_alloc(bytes.length);
  if (!ptr) {
    result.textContent = "Could not allocate WebAssembly memory for this source.";
    return;
  }

  try {
    new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
    const started = performance.now();
    const packed = wasm.demo_check(ptr, bytes.length);
    const elapsed = performance.now() - started;
    const mask = (1n << 25n) - 1n;
    const code = Number(packed >> 50n);
    const start = Number((packed >> 25n) & mask);
    const end = Number(packed & mask);
    const name = reasonNames[code] ?? `Reason ${code}`;
    const lines = source.value.split("\n").length - Number(source.value.endsWith("\n"));
    const summary = `${bytes.length.toLocaleString()} UTF-8 bytes, ${lines} lines · Wasm scan ${elapsed.toFixed(3)} ms`;

    result.dataset.state = code === 0 ? "pass" : "reject";
    result.textContent = code === 0
      ? `Pass: within default screening limits.\n${summary}`
      : `${name}: rejected at byte range ${start}..${end}.\n${summary}`;
  } finally {
    wasm.demo_free(ptr, bytes.length);
  }
}

checkButton.addEventListener("click", screen);
async function loadSample(name) {
  const response = await fetch(`./samples/${name}.py`);
  if (!response.ok) throw new Error(`sample HTTP ${response.status}`);
  source.value = await response.text();
  screen();
}

normalButton.addEventListener("click", () => loadSample("normal").catch(showError));
nestedButton.addEventListener("click", () => loadSample("deep_nesting").catch(showError));

function showError(error) {
  result.dataset.state = "reject";
  result.textContent = `Could not load demo: ${error.message}`;
}

try {
  const response = await fetch("./tokspam_demo.wasm");
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  ({ instance: { exports: wasm } } = await WebAssembly.instantiate(await response.arrayBuffer()));
  checkButton.disabled = false;
  checkButton.textContent = "Screen source";
  normalButton.disabled = false;
  nestedButton.disabled = false;
  await loadSample("normal");
} catch (error) {
  showError(error);
  checkButton.textContent = "WebAssembly unavailable";
}
