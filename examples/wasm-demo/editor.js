const MAX_HIGHLIGHT_CHARS = 100_000;
const MAX_NUMBERED_LINES = 5_000;
const keywords = new Set([
  "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
  "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
  "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
  "return", "try", "while", "with", "yield", "match", "case",
]);
const constants = new Set(["True", "False", "None"]);
const tokens = /(?:[rRuUbBfF]{0,2})?(?:"""[\s\S]*?(?:"""|$)|'''[\s\S]*?(?:'''|$)|"(?:\\[\s\S]|[^"\\])*"?|'(?:\\[\s\S]|[^'\\])*'?)|#[^\n]*|\b(?:[A-Za-z_]\w*|\d+(?:\.\d+)?)\b/g;

function escapeHtml(text) {
  return text.replace(/[&<>]/g, char => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[char]);
}

function highlight(source) {
  let html = "";
  let previous = 0;

  for (const match of source.matchAll(tokens)) {
    const token = match[0];
    let kind;

    if (token.startsWith("#")) kind = "comment";
    else if (/^[rRuUbBfF]{0,2}['"]/.test(token)) kind = "string";
    else if (keywords.has(token)) kind = "keyword";
    else if (constants.has(token)) kind = "constant";
    else if (/^\d/.test(token)) kind = "number";

    html += escapeHtml(source.slice(previous, match.index));
    html += kind ? `<span class="tok-${kind}">${escapeHtml(token)}</span>` : escapeHtml(token);
    previous = match.index + token.length;
  }

  return html + escapeHtml(source.slice(previous)) + "\n";
}

export function createEditor(source, editor, codeLayer, lineNumbers) {
  function syncScroll() {
    codeLayer.scrollTop = source.scrollTop;
    codeLayer.scrollLeft = source.scrollLeft;
    lineNumbers.scrollTop = source.scrollTop;
  }

  source.addEventListener("scroll", syncScroll);
  source.addEventListener("keydown", event => {
    if (event.key !== "Tab" || event.shiftKey) return;

    event.preventDefault();
    source.setRangeText("    ", source.selectionStart, source.selectionEnd, "end");
    source.dispatchEvent(new Event("input", { bubbles: true }));
  });

  function render() {
    const value = source.value;
    let lines = 1;

    if (value.length <= MAX_HIGHLIGHT_CHARS) {
      for (const char of value) {
        if (char === "\n") lines++;
        if (lines > MAX_NUMBERED_LINES) break;
      }
    }

    const plain = value.length > MAX_HIGHLIGHT_CHARS || lines > MAX_NUMBERED_LINES;
    editor.dataset.plain = String(plain);

    // Keep very large inputs in the native textarea so decoration cannot dominate screening work.
    if (plain) {
      codeLayer.textContent = "";
      lineNumbers.textContent = "";
    } else {
      codeLayer.innerHTML = highlight(value);
      lineNumbers.textContent = Array.from({ length: lines }, (_, index) => index + 1).join("\n");
      syncScroll();
    }

    return plain;
  }

  render();
  document.documentElement.classList.add("editor-ready");
  return render;
}
