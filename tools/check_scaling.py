"""Measure how Spam runtime scales across generated source families."""

import argparse
import json
import math
import subprocess
from pathlib import Path


def generate(family, size):
    if family == "flat":
        return "".join(f"v{i} = {i}\n" for i in range(size))
    if family == "operators":
        return "x = " + "+".join("1" for _ in range(size)) + "\n"
    if family == "fstrings":
        return "x = f'" + "{x}" * size + "'\n"
    if family == "annotations":
        return "".join(f"v{i}: 'list[int]' = []\n" for i in range(size))
    raise ValueError(f"unknown family: {family}")


def measure(binary, source):
    output = subprocess.check_output(
        [str(binary), "--reuse"], input=source, text=True, timeout=30
    ).strip()
    _, status, source_bytes, nanoseconds = output.split(",")
    return {"status": status, "bytes": int(source_bytes), "nanoseconds": float(nanoseconds)}


def exponent(samples):
    xs = [math.log(sample["bytes"]) for sample in samples]
    ys = [math.log(max(sample["nanoseconds"], 1.0)) for sample in samples]
    x_mean = sum(xs) / len(xs)
    y_mean = sum(ys) / len(ys)
    return sum((x - x_mean) * (y - y_mean) for x, y in zip(xs, ys)) / sum(
        (x - x_mean) ** 2 for x in xs
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--assert-linear", action="store_true")
    parser.add_argument("--maximum-exponent", type=float, default=1.30)
    args = parser.parse_args()

    report = {"families": {}}
    for family in ("flat", "operators", "fstrings", "annotations"):
        samples = [measure(args.benchmark.resolve(), generate(family, size)) for size in (256, 512, 1024, 2048, 4096)]
        fitted = exponent(samples)
        report["families"][family] = {"samples": samples, "log_log_exponent": fitted}

    text = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    print(text, end="")
    if args.assert_linear and any(
        result["log_log_exponent"] > args.maximum_exponent for result in report["families"].values()
    ):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
