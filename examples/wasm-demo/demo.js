import { createEditor } from "./editor.js";

const reasonNames = [
  "Pass", "Source size", "Token count", "Token size", "Nesting", "Indentation",
  "AST depth", "Lexical structure", "Quoted annotation", "Storage",
  "Call arguments", "Parameters", "Display items", "Slice items",
  "Interpolation fields", "Inconclusive", "Accounting overflow", "Invalid UTF-8", "Lexical depth",
];

const validationDetails = {
  1: ["Source byte size", "The source is larger than the configured byte budget."],
  2: ["Token count", "The streaming lexer emitted more tokens than the configured budget."],
  3: ["Token byte size", "A single token exceeded its configured byte budget."],
  4: ["Parser nesting estimate", "The nesting estimate exceeded its budget. Nested delimiters and constructs such as calls can contribute."],
  5: ["Indentation depth", "The number of active indentation levels exceeded its budget."],
  6: ["AST depth estimate", "The estimated AST depth exceeded its budget. Long operator or attribute chains can contribute."],
  7: ["Lexical structure", "The scanner found malformed or unsupported lexical structure near this range."],
  8: ["Quoted annotation screening", "A quoted annotation needed more nested screening than allowed, or its contents failed a structural check. The range points to the outer string."],
  9: ["Scratch storage", "The scan could not reserve enough scratch memory to continue."],
  16: ["Accounting overflow", "The scan could not continue with representable counters."],
  18: ["Lexical depth", "The active lexer context depth exceeded its configured threshold."],
  10: ["Call argument count", "A call contained more arguments than its configured budget."],
  11: ["Parameter count", "A definition contained more parameters than its configured budget."],
  12: ["Display item count", "A display contained more items than its configured budget."],
  13: ["Slice item count", "A slice contained more items than its configured budget."],
  14: ["Interpolation field count", "A string contained more interpolation fields than its configured budget."],
  15: ["Incomplete screening", "Tokspam stopped without confirming a structural violation. A work budget or bounded tracker may have been exhausted."],
  17: ["UTF-8 input", "The input was not valid UTF-8."],
};

const source = document.querySelector("#source");
const sourceStatus = document.querySelector("#source-status");
const result = document.querySelector("#result");
const resultTitle = document.querySelector("#result-title");
const resultDetail = document.querySelector("#result-detail");
const resultMeta = document.querySelector("#result-meta");
const resultInsights = document.querySelector("#result-insights");
const resultRule = document.querySelector("#result-rule");
const resultLimit = document.querySelector("#result-limit");
const resultLocation = document.querySelector("#result-location");
const resultContext = document.querySelector("#result-context");
const resultExcerpt = document.querySelector("#result-excerpt code");
const jumpButton = document.querySelector("#jump");
const checkButton = document.querySelector("#check");
const sampleButtons = [...document.querySelectorAll("[data-sample]")];
const resetLimitsButton = document.querySelector("#reset-limits");
const limitsNote = document.querySelector("#limits-note");
const limitsError = document.querySelector("#limits-error");
const limitInputs = [
  document.querySelector("#limit-source"),
  document.querySelector("#limit-tokens"),
  document.querySelector("#limit-nesting"),
  document.querySelector("#limit-indentation"),
  document.querySelector("#limit-ast"),
  document.querySelector("#limit-annotations"),
];
const encoder = new TextEncoder();
const renderEditor = createEditor(
  source,
  document.querySelector("#editor"),
  document.querySelector("#code-layer"),
  document.querySelector("#line-numbers"),
);
let wasm;
let rejectionSelection;
let defaultSettings;
let sampleLoadSequence = 0;

function showResult(state, title, detail, meta = "") {
  resultInsights.hidden = true;
  resultContext.hidden = true;
  rejectionSelection = undefined;
  result.dataset.state = state;
  resultTitle.textContent = title;
  resultDetail.textContent = detail;
  resultMeta.textContent = meta;

  if (state !== "idle" && state !== "reject") {
    result.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }
}

function formatLimit(code, limit) {
  if (limit === null) return "No single numeric threshold";
  if (code === 1 || code === 3) return `${limit.toLocaleString()} UTF-8 bytes`;
  if (code === 2) return `${limit.toLocaleString()} tokens`;
  if (code === 5) return `${limit.toLocaleString()} indentation levels`;
  if (code === 4 || code === 6) return `${limit.toLocaleString()} depth units`;
  if (code === 8) return `${limit.toLocaleString()} nested annotation rescans`;
  return `${limit.toLocaleString()} items`;
}

function configuredLimit(code, settings) {
  const selected = {
    1: settings[0],
    2: settings[1],
    4: settings[2],
    5: settings[3],
    6: settings[4],
    8: settings[5],
  }[code];

  if (selected !== undefined) return selected;

  const fallback = wasm.demo_default_limit(code);
  return fallback || null;
}

function isDefaultSettings(settings) {
  return settings.every((value, index) => value === defaultSettings[index]);
}

function updateLimitsNote() {
  const values = limitInputs.map((input, index) => input.valueAsNumber * (index === 0 ? 1024 : 1));
  const valid = limitInputs.every(input => input.checkValidity() && Number.isSafeInteger(input.valueAsNumber));
  limitsNote.textContent = valid
    ? `${isDefaultSettings(values) ? "Default limits" : "Custom limits"} · Work budget: ${(values[0] + values[1]).toLocaleString()} units (source bytes + tokens). Other limits stay at their defaults.`
    : "Use whole numbers within the allowed range shown in each field.";
}

function readSettings() {
  const invalid = limitInputs.find(input => !input.checkValidity() || !Number.isSafeInteger(input.valueAsNumber));
  if (invalid) {
    limitsError.textContent = `${invalid.labels[0].textContent.trim()} must be a whole number from 0 to ${Number(invalid.max).toLocaleString()}.`;
    limitsError.hidden = false;
    const details = invalid.closest("details");
    if (details) details.open = true;
    invalid.focus();
    return null;
  }

  limitsError.hidden = true;
  return limitInputs.map((input, index) => input.valueAsNumber * (index === 0 ? 1024 : 1));
}

function restoreDefaultLimits() {
  limitInputs.forEach((input, index) => {
    const unit = index === 0 ? 1024 : 1;
    input.value = wasm.demo_setting_default(index) / unit;
    input.max = wasm.demo_setting_maximum(index) / unit;
    input.disabled = false;
  });
  defaultSettings = limitInputs.map((input, index) => input.valueAsNumber * (index === 0 ? 1024 : 1));
  limitsError.hidden = true;
  updateLimitsNote();
  showResult("idle", "Not checked yet", source.value ? "Press Check limits to use the restored defaults." : "Load an example or enter source, then press Check limits.");
}

function showSourceLocation(bytes, start, end) {
  if (start > end || end > bytes.length) {
    resultLocation.textContent = `UTF-8 bytes ${start}..${end}`;
    return;
  }

  const decoder = new TextDecoder();
  const startIndex = decoder.decode(bytes.subarray(0, start)).length;
  const endIndex = decoder.decode(bytes.subarray(0, end)).length;
  const text = source.value;
  const lineStart = text.lastIndexOf("\n", startIndex - 1) + 1;
  const lineEnd = text.indexOf("\n", startIndex);
  const lineStop = lineEnd === -1 ? text.length : lineEnd;
  let line = 1;
  let column = 1;

  for (const char of text.slice(0, startIndex)) {
    if (char === "\n") line++;
  }
  for (const char of text.slice(lineStart, startIndex)) {
    column++;
  }

  resultLocation.textContent = `Line ${line}, column ${column} · bytes ${start}..${end}`;

  const beforeStart = Math.max(lineStart, startIndex - 50);
  const markEnd = Math.min(Math.max(endIndex, startIndex + 1), lineStop, startIndex + 30);
  const afterStart = Math.min(endIndex, lineStop);
  const prefix = `${beforeStart > lineStart ? "…" : ""}${text.slice(beforeStart, startIndex)}`;
  const marked = text.slice(startIndex, markEnd) || "↳";
  const suffix = `${markEnd < afterStart ? "…" : ""}${text.slice(afterStart, Math.min(lineStop, afterStart + 50))}${afterStart + 50 < lineStop ? "…" : ""}`;
  const mark = document.createElement("mark");
  mark.textContent = marked;
  resultExcerpt.replaceChildren(document.createTextNode(prefix), mark, document.createTextNode(suffix));
  resultContext.hidden = false;
  rejectionSelection = { startIndex, endIndex };
}

function showRejection(code, start, end, bytes, meta, settings) {
  const [rule, explanation] = validationDetails[code] ?? [reasonNames[code] ?? `Reason ${code}`, "Tokspam rejected this source near the reported range."];
  const limit = configuredLimit(code, settings);
  const limitText = formatLimit(code, limit);

  const title = [9, 15, 16].includes(code) ? "Screening incomplete" : `${reasonNames[code] ?? "Source"} rejected`;
  showResult("reject", title, explanation, meta);
  resultRule.textContent = rule;
  resultLimit.textContent = limitText;
  resultInsights.hidden = false;

  if (code === 1 || code === 9) {
    resultLocation.textContent = code === 1 ? "Entire source" : "Before scanning";
  } else {
    showSourceLocation(bytes, start, end);
  }

  result.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

function updateSource(label, selectedSample = null) {
  const length = source.value.length;
  const plain = renderEditor();
  sampleButtons.forEach(button => button.setAttribute("aria-pressed", String(button.dataset.sample === selectedSample)));
  sourceStatus.textContent = length
    ? `${label} · ${length.toLocaleString()} characters${plain ? " · highlighting paused for large source" : ""}`
    : "No source loaded";
  checkButton.disabled = !wasm || !source.value.trim();
  showResult(
    "idle",
    "Not checked yet",
    length ? "Press Check limits to check this source." : "Load an example or enter source, then press Check limits.",
  );
}

function screen() {
  const settings = readSettings();
  if (!settings) return;

  const bytes = encoder.encode(source.value);
  const ptr = wasm.demo_alloc(bytes.length);
  if (!ptr) {
    showResult("error", "Could not run screening", "WebAssembly memory allocation failed.");
    return;
  }

  try {
    new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
    const started = performance.now();
    const code = wasm.demo_check_configured(ptr, bytes.length, ...settings);
    const packed = wasm.demo_result_range();
    const elapsed = performance.now() - started;
    const mask = (1n << 32n) - 1n;
    const start = Number(packed >> 32n);
    const end = Number(packed & mask);
    const lines = source.value.split("\n").length - Number(source.value.endsWith("\n"));
    const timing = elapsed === 0 ? "below browser timer resolution" : `${elapsed.toFixed(3)} ms`;
    const meta = `${bytes.length.toLocaleString()} UTF-8 bytes · ${lines} lines · ${isDefaultSettings(settings) ? "Default" : "Custom"} limits · Wasm scan ${timing}`;

    if (code === 0) {
      showResult("pass", "Within configured limits", `No configured resource limit was exceeded. This check used parser nesting ${settings[2]}, estimated AST depth ${settings[4]}, and indentation ${settings[3]}.`, meta);
    } else {
      showRejection(code, start, end, bytes, meta, settings);
    }
  } finally {
    wasm.demo_free(ptr, bytes.length);
  }
}

async function loadSample(button) {
  const sequence = ++sampleLoadSequence;
  const response = await fetch(`./samples/${button.dataset.sample}.py`);
  if (sequence !== sampleLoadSequence) return;
  if (!response.ok) throw new Error(`sample HTTP ${response.status}`);
  const sample = await response.text();

  // A later click wins if two sample requests finish out of order.
  if (sequence !== sampleLoadSequence) return;

  restoreDefaultLimits();
  source.value = sample;
  updateSource(button.dataset.label, button.dataset.sample);
}

function showError(error) {
  showResult("error", "Demo unavailable", error.message);
}

checkButton.addEventListener("click", screen);
resetLimitsButton.addEventListener("click", restoreDefaultLimits);
limitInputs.forEach(input => input.addEventListener("input", () => {
  limitsError.hidden = true;
  updateLimitsNote();
  showResult("idle", "Not checked yet", "Limits changed. Press Check limits to run again.");
}));
jumpButton.addEventListener("click", () => {
  if (!rejectionSelection) return;

  source.focus({ preventScroll: true });
  source.setSelectionRange(rejectionSelection.startIndex, rejectionSelection.endIndex);
  source.scrollIntoView({ behavior: "smooth", block: "center" });
});
source.addEventListener("input", () => {
  sampleLoadSequence++;
  updateSource("Edited source");
});
sampleButtons.forEach(button => button.addEventListener("click", () => loadSample(button).catch(showError)));

try {
  const response = await fetch("./tokspam_demo.wasm");
  if (!response.ok) throw new Error(`WebAssembly HTTP ${response.status}`);
  ({ instance: { exports: wasm } } = await WebAssembly.instantiate(await response.arrayBuffer()));
  restoreDefaultLimits();
  checkButton.textContent = "Check limits";
  checkButton.disabled = !source.value.trim();
  sampleButtons.forEach(button => { button.disabled = false; });
  resetLimitsButton.disabled = false;
} catch (error) {
  showError(error);
  checkButton.textContent = "WebAssembly unavailable";
}
