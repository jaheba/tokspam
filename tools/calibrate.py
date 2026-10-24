"""Measure valid Python files for parser-preflight limit calibration."""

import argparse
import ast
import hashlib
import io
import json
import sqlite3
import statistics
import subprocess
import sys
import sysconfig
import time
import tokenize
import warnings
from pathlib import Path

SCHEMA = Path(__file__).resolve().parent.parent / "corpus" / "schema.sql"


def median_seconds(action, repeats):
    samples = []
    for _ in range(repeats):
        start = time.perf_counter_ns()
        action()
        samples.append((time.perf_counter_ns() - start) / 1_000_000_000)
    return samples, statistics.median(samples)


def compile_source(source, filename):
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", SyntaxWarning)
        return compile(source, filename, "exec")


def positional_parameters(arguments):
    return len(arguments.posonlyargs) + len(arguments.args) + len(arguments.kwonlyargs)


def interpolation_fields(node):
    field_types = (ast.FormattedValue,)
    if hasattr(ast, "Interpolation"):
        field_types += (ast.Interpolation,)
    return sum(isinstance(value, field_types) for value in node.values)


def source_metrics(tree):
    metrics = {
        "ast_nodes": 0,
        "statement_count": 0,
        "identifiers": set(),
        "identifier_occurrences": 0,
        "max_call_arguments": 0,
        "max_parameters": 0,
        "max_display_items": 0,
        "max_slice_items": 0,
        "max_interpolation_fields": 0,
    }
    stack = [tree]
    while stack:
        node = stack.pop()
        metrics["ast_nodes"] += 1
        metrics["statement_count"] += isinstance(node, ast.stmt)
        if isinstance(node, ast.Name):
            metrics["identifiers"].add(node.id)
            metrics["identifier_occurrences"] += 1
        elif isinstance(node, ast.arg):
            metrics["identifiers"].add(node.arg)
            metrics["identifier_occurrences"] += 1
        elif isinstance(node, ast.alias):
            metrics["identifiers"].add(node.asname or node.name)
            metrics["identifier_occurrences"] += 1
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            metrics["identifiers"].add(node.name)
            metrics["identifier_occurrences"] += 1
            count = positional_parameters(node.args)
            count += bool(node.args.vararg) + bool(node.args.kwarg)
            metrics["max_parameters"] = max(metrics["max_parameters"], count)
        elif isinstance(node, ast.ClassDef):
            metrics["identifiers"].add(node.name)
            metrics["identifier_occurrences"] += 1
        elif isinstance(node, ast.Attribute):
            metrics["identifiers"].add(node.attr)
            metrics["identifier_occurrences"] += 1
        elif isinstance(node, ast.keyword) and node.arg is not None:
            metrics["identifiers"].add(node.arg)
            metrics["identifier_occurrences"] += 1

        if isinstance(node, ast.Lambda):
            count = positional_parameters(node.args)
            count += bool(node.args.vararg) + bool(node.args.kwarg)
            metrics["max_parameters"] = max(metrics["max_parameters"], count)

        if isinstance(node, ast.Call):
            metrics["max_call_arguments"] = max(
                metrics["max_call_arguments"], len(node.args) + len(node.keywords)
            )
        elif isinstance(node, (ast.List, ast.Tuple, ast.Set)):
            metrics["max_display_items"] = max(metrics["max_display_items"], len(node.elts))
        elif isinstance(node, ast.Dict):
            metrics["max_display_items"] = max(metrics["max_display_items"], len(node.keys))
        elif isinstance(node, ast.Subscript) and isinstance(node.slice, ast.Tuple):
            metrics["max_slice_items"] = max(metrics["max_slice_items"], len(node.slice.elts))
        elif isinstance(node, (ast.JoinedStr, getattr(ast, "TemplateStr", ast.JoinedStr))):
            metrics["max_interpolation_fields"] = max(
                metrics["max_interpolation_fields"], interpolation_fields(node)
            )
        stack.extend(ast.iter_child_nodes(node))

    metrics["identifier_count"] = len(metrics.pop("identifiers"))
    return metrics


def count_tokens(source):
    reader = io.StringIO(source).readline
    return sum(token.type not in {tokenize.ENCODING, tokenize.ENDMARKER} for token in tokenize.generate_tokens(reader))


def probe(command, source, timeout):
    start = time.monotonic()
    try:
        result = subprocess.run(command, input=source.encode(), capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return "timeout", timeout
    output = result.stdout.decode(errors="replace").strip()
    if result.returncode:
        return "error", time.monotonic() - start
    return output, time.monotonic() - start


def iter_sources(roots, maximum_bytes):
    for corpus, root in roots:
        for path in sorted(root.rglob("*.py")):
            excluded = {"__pycache__", ".git", ".venv", ".worktrees", "target", "node_modules"}
            if excluded.intersection(path.parts) or (corpus == "stdlib" and "site-packages" in path.parts):
                continue
            try:
                data = path.read_bytes()
            except OSError:
                continue
            if len(data) <= maximum_bytes:
                yield corpus, root, path, data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", action="append", type=Path, default=[])
    parser.add_argument("--root-name", action="append", default=[])
    parser.add_argument("--stdlib", action="store_true")
    parser.add_argument("--spam-probe", type=Path, required=True)
    parser.add_argument("--ruff-probe", type=Path)
    parser.add_argument("--database", type=Path, default=Path("corpus/cases.sqlite"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--maximum-bytes", type=int, default=1024 * 1024)
    parser.add_argument("--timeout", type=float, default=0.5)
    args = parser.parse_args()
    if len(args.root_name) not in {0, len(args.root)}:
        parser.error("provide one --root-name for every --root, or omit all names")
    if args.repeats < 1 or args.maximum_bytes < 1 or args.timeout <= 0:
        parser.error("repeats, maximum bytes, and timeout must be positive")

    roots = []
    if args.stdlib:
        roots.append(("stdlib", Path(sysconfig.get_path("stdlib"))))
    names = args.root_name or [path.name for path in args.root]
    roots.extend(zip(names, args.root, strict=True))
    if not roots:
        parser.error("select --stdlib or at least one --root")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.database.parent.mkdir(parents=True, exist_ok=True)
    spam_probe = args.spam_probe.resolve()
    ruff_probe = args.ruff_probe.resolve() if args.ruff_probe else None
    observations = []
    skipped = {"decode": 0, "syntax": 0, "compile": 0, "tokenize": 0}
    for corpus, root, path, data in iter_sources(roots, args.maximum_bytes):
        try:
            source = data.decode("utf-8")
        except UnicodeDecodeError:
            skipped["decode"] += 1
            continue
        try:
            tree = ast.parse(source, filename=str(path))
        except SyntaxError:
            skipped["syntax"] += 1
            continue
        try:
            compile_source(source, str(path))
        except (SyntaxError, ValueError):
            skipped["compile"] += 1
            continue
        try:
            token_count = count_tokens(source)
        except (IndentationError, tokenize.TokenError):
            skipped["tokenize"] += 1
            continue

        metrics = source_metrics(tree)
        parse_samples, parse_median = median_seconds(lambda: ast.parse(source), args.repeats)
        compile_samples, compile_median = median_seconds(
            lambda: compile_source(source, str(path)), args.repeats
        )
        spam_status, _ = probe([str(spam_probe), "check", "100"], source, args.timeout)
        ruff_status = None
        ruff_seconds = None
        if ruff_probe:
            ruff_status, ruff_seconds = probe([str(ruff_probe)], source, args.timeout)
        observation = {
            "corpus": corpus,
            "path": str(path.relative_to(root)),
            "source_sha256": hashlib.sha256(data).hexdigest(),
            "source_bytes": len(data),
            "token_count": token_count,
            **metrics,
            "spam_status": spam_status,
            "parse_samples": parse_samples,
            "parse_median_seconds": parse_median,
            "compile_samples": compile_samples,
            "compile_median_seconds": compile_median,
            "ruff_status": ruff_status,
            "ruff_seconds": ruff_seconds,
        }
        observations.append(observation)

    metadata = {
        "format": "spam-calibration-v1",
        "python_version": sys.version,
        "roots": [{"name": name, "path": str(path.resolve())} for name, path in roots],
        "repeats": args.repeats,
        "maximum_bytes": args.maximum_bytes,
        "timeout_seconds": args.timeout,
        "files": len(observations),
        "skipped": skipped,
    }
    args.output.write_text(json.dumps({"metadata": metadata, "observations": observations}, indent=2) + "\n")

    with sqlite3.connect(args.database) as database:
        database.executescript(SCHEMA.read_text())
        columns = {row[1] for row in database.execute("PRAGMA table_info(calibration_observations)")}
        if "identifier_occurrences" not in columns:
            database.execute(
                "ALTER TABLE calibration_observations ADD COLUMN identifier_occurrences INTEGER NOT NULL DEFAULT 0"
            )
        if "ruff_status" not in columns:
            database.execute("ALTER TABLE calibration_observations ADD COLUMN ruff_status TEXT")
        with database:
            run_id = database.execute(
                "INSERT INTO runs (metadata) VALUES (?)", (json.dumps(metadata),)
            ).lastrowid
            database.executemany(
                "INSERT INTO calibration_observations "
                "(run_id, corpus, path, source_sha256, source_bytes, token_count, ast_nodes, statement_count, "
                "identifier_count, identifier_occurrences, max_call_arguments, max_parameters, max_display_items, "
                "max_slice_items, "
                "max_interpolation_fields, spam_status, parse_samples, parse_median_seconds, compile_samples, "
                "compile_median_seconds, ruff_status, ruff_seconds) "
                "VALUES (:run_id, :corpus, :path, :source_sha256, :source_bytes, :token_count, :ast_nodes, "
                ":statement_count, :identifier_count, :identifier_occurrences, :max_call_arguments, :max_parameters, "
                ":max_display_items, :max_slice_items, :max_interpolation_fields, :spam_status, :parse_samples_json, "
                ":parse_median_seconds, :compile_samples_json, :compile_median_seconds, :ruff_status, :ruff_seconds)",
                [
                    {
                        **observation,
                        "run_id": run_id,
                        "parse_samples_json": json.dumps(observation["parse_samples"]),
                        "compile_samples_json": json.dumps(observation["compile_samples"]),
                    }
                    for observation in observations
                ],
            )
    print(json.dumps({"run_id": run_id, "files": len(observations), "skipped": skipped}))


if __name__ == "__main__":
    main()
