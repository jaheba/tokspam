"""Compare Spam's experimental construct-width boundaries with CPython's AST."""

import argparse
import ast
import subprocess
from pathlib import Path

from calibrate import source_metrics


METRICS = {
    "max_call_arguments": ("CallArguments", 0),
    "max_parameters": ("Parameters", 1),
    "max_display_items": ("DisplayItems", 2),
    "max_slice_items": ("SliceItems", 3),
    "max_interpolation_fields": ("InterpolationFields", 4),
}


def cases():
    yield "calls-positional", "result = f(a, b, c)"
    yield "calls-keywords", "result = f(a=1, b=2, **values)"
    yield "calls-unpacking", "result = f(*left, middle, *right)"
    yield "calls-generator", "result = f(x for x in values)"
    yield "calls-chained", "result = factory()(a, b)"
    yield "calls-literal-attribute", "result = 'x'.format(a, b)"
    yield "class-bases", "class C(A, B, metaclass=M):\n    pass"
    yield "function-all-parameters", "def f(a, /, b=1, *args, c, **kwargs): pass"
    yield "function-bare-star", "def f(a, *, b, c): pass"
    yield "async-function", "async def f(a, b, *, c): pass"
    yield "lambda-all-parameters", "value = lambda a, /, b=1, *args, c, **kwargs: a"
    yield "lambda-bare-star", "value = lambda a, *, b, c: a"
    yield "list-display", "value = [a, *middle, c]"
    yield "tuple-display", "value = (a, b, c)"
    yield "set-display", "value = {a, *middle, c}"
    yield "dict-display", "value = {'a': 1, **middle, 'c': 3}"
    yield "comprehension", "value = [x for x in values if x]"
    yield "grouping", "value = (((item)))"
    yield "import-parentheses", "from package import (a, b, c)"
    yield "slice-tuple", "value = items[1:2, ..., key]"
    yield "slice-starred", "value = items[*left, middle, *right]"
    yield "slice-single", "value = items[1:2]"
    yield "fstring-flat", "value = f'{a!r}{b=}{c:>10}'"
    yield "fstring-format-fields", "value = f'{value:{width}.{precision}f}'"
    yield "fstring-separate", "value = f'{a}' f'{b}' f'{c}'"
    yield "fstring-nested-expression", "value = f\"{f'{a}{b}'}{c}\""
    if hasattr(ast, "TemplateStr"):
        yield "template-flat", "value = t'{a}{b}{c}'"


def run_probe(probe, source, selected, limit):
    widths = ["-"] * 5
    widths[selected] = str(limit)
    result = subprocess.run(
        [probe, "check-widths", "100", *widths],
        input=source.encode(),
        capture_output=True,
        check=True,
    )
    return result.stdout.decode().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spam-probe", type=Path, required=True)
    args = parser.parse_args()
    failures = []
    checked = 0
    for name, source in cases():
        metrics = source_metrics(ast.parse(source))
        for metric, (reason, selected) in METRICS.items():
            width = metrics[metric]
            at_limit = run_probe(args.spam_probe, source, selected, width)
            checked += 1
            if at_limit != "accept":
                failures.append((name, metric, width, "accept", at_limit))
            if width:
                below = run_probe(args.spam_probe, source, selected, width - 1)
                checked += 1
                if not below.startswith(f"reject:{reason}:"):
                    failures.append((name, metric, width - 1, reason, below))

    for failure in failures:
        print(": ".join(map(str, failure)))
    print(f"checked {checked} boundaries across {sum(1 for _ in cases())} sources; {len(failures)} mismatches")
    raise SystemExit(bool(failures))


if __name__ == "__main__":
    main()
