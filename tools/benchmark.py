"""Compare in-process Spam screening with Ruff and CPython parsing."""

import argparse
import ast
import json
import statistics
import subprocess
import sysconfig
import time
from pathlib import Path


def cpython_ns(source):
    for _ in range(3):
        ast.parse(source)
    iterations = 1
    while True:
        start = time.perf_counter_ns()
        for _ in range(iterations):
            ast.parse(source)
        if time.perf_counter_ns() - start >= 30_000_000 or iterations >= 1_000_000:
            break
        iterations *= 2
    samples = []
    for _ in range(7):
        start = time.perf_counter_ns()
        for _ in range(iterations):
            ast.parse(source)
        samples.append((time.perf_counter_ns() - start) / iterations)
    return statistics.median(samples)


def rust_ns(binary, source, bench_flag=None):
    command = [str(binary)] + ([bench_flag] if bench_flag else [])
    result = subprocess.run(command, input=source.encode(), capture_output=True, check=True)
    implementation, status, size, ns = result.stdout.decode().strip().split(',')
    if int(size) != len(source.encode()):
        raise ValueError(f'{implementation} measured an unexpected source size')
    return {'ns': float(ns), 'status': status}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spam-bench', type=Path, required=True)
    parser.add_argument('--ruff-probe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    stdlib = Path(sysconfig.get_path('stdlib'))
    sources = {
        'tiny': 'x = 1 + 2\n',
        'chain-80': 'x = ' + '1 + ' * 80 + '1\n',
        'flat-60k': 'x = 1\n' * 10_000,
        'shutil': (stdlib / 'shutil.py').read_text(),
        'chain-5000': 'x = ' + '1 + ' * 5000 + '1\n',
    }
    results = []
    for name, source in sources.items():
        spam = rust_ns(args.spam_bench.resolve(), source)
        ruff = rust_ns(args.ruff_probe.resolve(), source, '--bench')
        cpython = cpython_ns(source)
        results.append({'name': name, 'bytes': len(source.encode()), 'spam': spam,
                        'ruff_ns': ruff['ns'], 'cpython_ns': cpython})
        print(f'{name}: Spam {spam["ns"] / 1000:.1f} µs ({spam["status"]}), '
              f'Ruff {ruff["ns"] / 1000:.1f} µs, CPython {cpython / 1000:.1f} µs')
    args.output.write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    main()
