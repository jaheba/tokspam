import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SPAM = "/Users/jaheba/p/spam/target/release/examples/probe"
RUFF = "/Users/jaheba/p/spam/tools/ruff-probe/target/release/spam-ruff-probe"
PYREFLY = "/Users/jaheba/.cache/uv/archive-v0/VLLFmsg829jmqgDe/bin/pyrefly"
PYTHONS = ["/opt/homebrew/bin/python3.14", "/opt/homebrew/bin/python3.15"]
TIMEOUT = 0.5

CPYTHON_PROBE = r'''
import ast, sys, time
s = sys.stdin.buffer.read()
t = time.perf_counter()
try:
    tree = ast.parse(s, filename='<campaign-50>')
    if sys.argv[1] == 'all':
        sum(1 for _ in ast.walk(tree))
        compile(tree, '<campaign-50>', 'exec')
    del tree
except (SyntaxError, RecursionError, MemoryError) as e:
    status = type(e).__name__
else:
    status = 'ok'
print(status)
print((time.perf_counter() - t) * 1000)
'''


def join(item, n, sep=","):
    return sep.join(item(i) for i in range(n))


def sources():
    n = 49_000
    yield 1, "late-call-keyword", "f(" + "x," * n + "if)\n"
    yield 2, "late-call-literal", "f(" + "0," * n + "if)\n"
    yield 3, "late-duplicate-star-param", "def f(" + join(lambda i: f"x{i}", 20_000) + ",*,*):\n pass\n"
    yield 4, "late-slash-param", "def f(" + join(lambda i: f"x{i}", 20_000) + ",/,/):\n pass\n"
    yield 5, "late-type-param", "def f[" + join(lambda i: f"T{i}", 20_000) + ",*]():\n pass\n"
    yield 6, "late-class-base", "class C(" + join(lambda i: "B", 49_000) + ",if):\n pass\n"
    yield 7, "late-import-alias", "import " + join(lambda i: f"m{i}", 24_000) + ", z as\n"
    yield 8, "late-from-import", "from m import (" + join(lambda i: f"x{i}", 24_000) + ", z as)\n"
    yield 9, "late-global", "global " + join(lambda i: f"x{i}", 24_000) + ", if\n"
    yield 10, "late-nonlocal", "def f():\n nonlocal " + join(lambda i: f"x{i}", 24_000) + ", if\n"
    yield 11, "late-duplicate-keyword", "f(" + join(lambda i: f"x{i}=0", 16_000) + ",x0=1)\n"
    yield 12, "late-dict-double-star", "{" + join(lambda i: f"{i}:0", 24_000) + ",**}\n"
    yield 13, "late-set-double-star", "{" + join(lambda i: str(i), 40_000) + ",**x}\n"
    yield 14, "late-list-comprehension", "[" + join(lambda i: str(i), 40_000) + " for]\n"
    yield 15, "late-tuple-hole", "(" + join(lambda i: "0", 49_000) + ",,)\n"
    yield 16, "semicolon-compound", "x=0;" * 24_000 + "if x: pass\n"
    yield 17, "lines-missing-suite", "x=0\n" * 24_000 + "if x:\n"
    yield 18, "elif-missing-colon", "if x:\n pass\n" + "elif x:\n pass\n" * 10_000 + "elif x\n pass\n"
    yield 19, "match-invalid-final-case", "match x:\n" + " case 0: pass\n" * 12_000 + " case if: pass\n"
    yield 20, "wide-mapping-pattern", "match x:\n case {" + join(lambda i: f"{i}: x{i}", 12_000) + ", []: y}: pass\n"
    yield 21, "wide-class-pattern", "match x:\n case C(" + join(lambda i: f"x{i}=y", 16_000) + ", z): pass\n"
    yield 22, "wide-sequence-pattern", "match x:\n case [" + join(lambda i: f"x{i}", 24_000) + ", *a, *b]: pass\n"
    yield 23, "late-or-pattern", "match x:\n case " + "0 | " * 24_000 + ": pass\n"
    yield 24, "late-comparison", "x = " + "a < " * 49_000 + "\n"
    yield 25, "late-boolean", "x = " + "a and " * 32_000 + "\n"
    yield 26, "late-additive", "x = " + "a + " * 49_000 + "\n"
    yield 27, "late-power", "x = " + "a ** " * 32_000 + "\n"
    yield 28, "late-attribute", "x = a" + ".a" * 49_000 + ".\n"
    yield 29, "late-slices", "x = a[" + "0:1," * 24_000 + ":]\n"
    yield 30, "late-conditional", "x = " + "a if b else " * 16_000 + "\n"
    yield 31, "lambda-no-body", "x = lambda " + join(lambda i: f"x{i}", 20_000) + ":\n"
    yield 32, "many-comprehension-filters", "x = [x for x in y " + "if x " * 24_000 + "if]\n"
    yield 33, "many-comprehension-generators", "x = [x " + "for x in y " * 16_000 + "for x in]\n"
    yield 34, "adjacent-strings", "x = " + "'a' " * 49_000 + "\n"
    yield 35, "adjacent-strings-unclosed", "x = " + "'a' " * 49_000 + "'\n"
    yield 36, "escape-heavy-bytes", "x = b'" + "\\x61" * 200_000 + "'\n"
    yield 37, "late-invalid-unicode-escape", "x = '" + "\\u0061" * 120_000 + "\\u0'\n"
    yield 38, "flat-fstring-fields", "x = f'" + "{x}" * 24_000 + "'\n"
    yield 39, "flat-fstring-unclosed-field", "x = f'" + "{x}" * 24_000 + "{'\n"
    yield 40, "flat-tstring-fields", "x = t'" + "{x}" * 24_000 + "'\n"
    yield 41, "adjacent-f-and-t-strings", "x = " + "f'{x}' t'{x}' " * 12_000 + "\n"
    yield 42, "unique-nfkc-identifiers", "".join(f"K{i}=0\n" for i in range(24_000))
    yield 43, "colliding-nfkc-identifiers", "".join(f"K{i % 10}=0\n" for i in range(24_000))
    yield 44, "underscored-decimal", "x = " + "1_" * 400_000 + "1\n"
    yield 45, "large-hexadecimal", "x = 0x" + "f" * 1_000_000 + "\n"
    yield 46, "malformed-hex-float", "x = 0x" + "f" * 900_000 + ".p+1\n"
    yield 47, "mixed-recursive-grammar", "x = " + "(lambda: a if b else " * 90 + "a" + " ** a)" * 90 + "\n"
    part = "@d\nclass C[T](B):\n x: list[T] = f(lambda: 0)\n"
    yield 48, "nested-rich-classes", part.replace("\n", "\n" + " " * 0) if False else nested_classes(90)
    yield 49, "deep-target-validation", "del " + "[" * 2_000 + "x" + "]" * 2_000 + "\n"
    yield 50, "large-ast-all-phases", "\n".join(f"x{i} = ({i}, {i + 1}, {i + 2})" for i in range(20_000)) + "\n"


def nested_classes(depth):
    lines = []
    for i in range(depth):
        indent = " " * i
        lines.extend((f"{indent}@d", f"{indent}class C{i}[T](B):", f"{indent} x: list[T] = f(lambda: 0)"))
    lines.append(" " * depth + "pass")
    return "\n".join(lines) + "\n"


def run(command, source=None, timeout=TIMEOUT):
    start = time.monotonic()
    try:
        result = subprocess.run(command, input=source, capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"status": "timeout", "wall_ms": round((time.monotonic() - start) * 1000, 2)}
    return {
        "status": "exit" if result.returncode else "ok",
        "returncode": result.returncode,
        "wall_ms": round((time.monotonic() - start) * 1000, 2),
        "stdout": result.stdout.decode(errors="replace")[-300:],
        "stderr": result.stderr.decode(errors="replace")[-300:],
    }


def main():
    output = Path(sys.argv[1])
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="spam-campaign50-") as tmp, output.open("w") as out:
        case_path = Path(tmp) / "case.py"
        for ident, name, source_text in sources():
            source = source_text.encode()
            if len(source) > 1024 * 1024:
                raise ValueError((ident, name, len(source)))
            case_path.write_bytes(source)
            record = {"id": ident, "name": name, "bytes": len(source)}
            record["spam"] = run([SPAM, "check", "100"], source)
            record["ruff"] = run([RUFF], source)
            for python in PYTHONS:
                key = Path(python).name
                record[key] = run([python, "-I", "-c", CPYTHON_PROBE, "all" if ident == 50 else "parse"], source)
            record["pyrefly"] = run([
                PYREFLY, "check", "--threads", "1", "--preset", "off", "--skip-interpreter-query",
                "--python-version", "3.14", "--summary", "none", "--progress-bar", "no",
                "--output-format", "omit-errors", str(case_path),
            ])
            out.write(json.dumps(record, sort_keys=True) + "\n")
            out.flush()
            compact = {k: v.get("status") + ":" + str(v.get("wall_ms")) for k, v in record.items() if isinstance(v, dict)}
            print(ident, name, len(source), compact, flush=True)


if __name__ == "__main__":
    main()
