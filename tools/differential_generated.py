"""Generate valid mixed Python and compare Spam widths with parser implementations."""

import argparse
import ast
import hashlib
import json
import random
import sqlite3
import subprocess
import tempfile
from pathlib import Path

from calibrate import source_metrics
from differential_widths import METRICS, run_probe

SCHEMA = Path(__file__).resolve().parent.parent / "corpus" / "schema.sql"


def expression(rng, width):
    names = ["a", "b", "c", "items", "value", "factory"]
    atoms = [rng.choice(names), str(rng.randrange(10)), f"'{rng.choice(names)}'"]
    value = f"({rng.choice(atoms)})"
    operations = [
        lambda x: f"({x} + {rng.choice(atoms)})",
        lambda x: f"{x}.{rng.choice(names)}",
        lambda x: f"{x}[{rng.randrange(3)}:{rng.randrange(3) + 3}]",
        lambda x: f"f({x}, *items, key={rng.choice(atoms)})",
        lambda x: f"[{x}, *items, {rng.choice(atoms)}]",
        lambda x: f"{{'x': {x}, **mapping}}",
        lambda x: f"(lambda p, /, q=1, *, r=2: {x})(1, 2, r=3)",
    ]
    for _ in range(width):
        value = rng.choice(operations)(value)
    return value


def generate(seed):
    rng = random.Random(seed)
    count = rng.randint(1, 6)
    body = [f"        v{i} = {expression(rng, rng.randint(0, 5))}" for i in range(count)]
    params = ["self", "a", "/", "b=1", "*args", "flag=True", "**kwargs"]
    rng.shuffle(body)
    bases = rng.sample(["Base", "Protocol", "Generic[T]"], rng.randint(1, 3))
    fields = "".join(f"{{v{i}}}" for i in range(count))
    return (
        "from __future__ import annotations\n"
        "from typing import Generic, Protocol, TypeVar\n"
        "T = TypeVar('T')\n"
        f"class C({', '.join(bases)}):\n"
        f"    def method({', '.join(params)}):\n"
        + "\n".join(body)
        + f"\n        return f'{fields}'\n"
        f"result = C().method({', '.join(str(i) for i in range(count))})\n"
    )


def parser_status(command, source):
    result = subprocess.run(command, input=source.encode(), capture_output=True, timeout=2)
    return result.stdout.decode(errors="replace").strip() or f"exit:{result.returncode}"


def pyrefly_status(command, source):
    if not command:
        return None
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "case.py"
        path.write_text(source)
        result = subprocess.run([*command, str(path)], capture_output=True, timeout=5)
        return "ok" if result.returncode in (0, 1) else f"exit:{result.returncode}"


def mismatch(probe, source):
    metrics = source_metrics(ast.parse(source))
    failures = []
    for metric, (reason, selected) in METRICS.items():
        width = metrics[metric]
        if run_probe(probe, source, selected, width) != "accept":
            failures.append(f"{metric}:at-limit")
        if width and not run_probe(probe, source, selected, width - 1).startswith(f"reject:{reason}:"):
            failures.append(f"{metric}:below-limit")
    return failures, metrics


def minimize(source, predicate):
    lines = source.splitlines(keepends=True)
    changed = True
    while changed and len(lines) > 1:
        changed = False
        for start in range(len(lines)):
            candidate = "".join(lines[:start] + lines[start + 1 :])
            try:
                ast.parse(candidate)
            except SyntaxError:
                continue
            if predicate(candidate):
                lines.pop(start)
                changed = True
                break
    return "".join(lines)


def store(database, metadata, records):
    with sqlite3.connect(database) as db:
        db.executescript(SCHEMA.read_text())
        with db:
            run_id = db.execute("INSERT INTO runs (metadata) VALUES (?)", (json.dumps(metadata),)).lastrowid
            for record in records:
                source = record["source"].encode()
                case_id = hashlib.sha256(source).hexdigest()
                db.execute(
                    "INSERT OR IGNORE INTO cases (id, source, family, seed, first_seen_run) VALUES (?, ?, ?, ?, ?)",
                    (case_id, source, "generated-mixed", str(record["seed"]), run_id),
                )
                for label in record["labels"]:
                    db.execute(
                        "INSERT OR IGNORE INTO case_labels (case_id, label, first_seen_run) VALUES (?, ?, ?)",
                        (case_id, label, run_id),
                    )
                for implementation, status in record["implementations"].items():
                    if status is not None:
                        db.execute(
                            "INSERT OR REPLACE INTO observations "
                            "(run_id, case_id, implementation, status) VALUES (?, ?, ?, ?)",
                            (run_id, case_id, implementation, status),
                        )
                        version = metadata.get("python") if implementation == "cpython" else None
                        phase = "screen" if implementation == "spam" else "parse"
                        db.execute(
                            "INSERT OR REPLACE INTO observation_context "
                            "(run_id, case_id, implementation, phase, implementation_version) "
                            "VALUES (?, ?, ?, ?, ?)",
                            (run_id, case_id, implementation, phase, version),
                        )
    return run_id


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spam-probe", type=Path, required=True)
    parser.add_argument("--ruff-probe", type=Path, required=True)
    parser.add_argument("--pyrefly-command", nargs="+")
    parser.add_argument("--cases", type=int, default=1000)
    parser.add_argument("--seed", type=int, default=20261024)
    parser.add_argument("--database", type=Path, default=Path("corpus/cases.sqlite"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    records = []
    mismatches = 0
    parser_failures = 0
    for index in range(args.cases):
        seed = args.seed + index
        source = generate(seed)
        ast.parse(source)
        failures, metrics = mismatch(args.spam_probe, source)
        implementations = {
            "cpython": "ok",
            "ruff": parser_status([str(args.ruff_probe)], source),
            "spam": "mismatch" if failures else "accept",
            "pyrefly": pyrefly_status(args.pyrefly_command, source),
        }
        labels = []
        if failures:
            source = minimize(source, lambda candidate: bool(mismatch(args.spam_probe, candidate)[0]))
            failures, metrics = mismatch(args.spam_probe, source)
            labels.append("differential-mismatch")
            mismatches += 1
        if implementations["ruff"] != "ok" or (
            implementations["pyrefly"] is not None and implementations["pyrefly"].startswith("exit:")
        ):
            labels.append("parser-failure")
            parser_failures += 1
        if labels:
            artifact = args.output / f"{hashlib.sha256(source.encode()).hexdigest()[:16]}.py"
            artifact.write_text(source)
            records.append({
                "seed": seed,
                "source": source,
                "metrics": metrics,
                "failures": failures,
                "labels": labels,
                "implementations": implementations,
                "artifact": artifact.name,
            })

    metadata = {
        "format": "spam-generated-differential-v1",
        "seed": args.seed,
        "cases": args.cases,
        "python": subprocess.check_output(["python3", "--version"], text=True).strip(),
        "pyrefly_command": args.pyrefly_command,
    }
    run_id = store(args.database, metadata, records)
    summary = {**metadata, "run_id": run_id, "retained": len(records), "width_mismatches": mismatches,
               "parser_failures": parser_failures}
    (args.output / "results.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records))
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    raise SystemExit(bool(mismatches))


if __name__ == "__main__":
    main()
