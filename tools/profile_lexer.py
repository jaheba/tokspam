"""Compare isolated lexer and reusable-checker throughput against saved binaries.

Build examples/lexer_benchmark and examples/benchmark in release mode before and
after a change. Each binary reports seven-sample medians; this driver repeats
measurements in opposite orders. Ratios below one mean faster scanning.
"""

import argparse
import json
import platform
import statistics
import subprocess
import sys
import sysconfig
from pathlib import Path


def workloads():
    stdlib = Path(sysconfig.get_path('stdlib'))
    modules = ['shutil.py', 'ast.py', 'dataclasses.py', 'functools.py',
               'json/decoder.py', 're/__init__.py']
    sources = {name: (stdlib / name).read_text() for name in modules}
    sources.update({
        'identifiers': '    long_identifier = other_identifier + third_identifier\n' * 1000,
        'comments': ('# ' + 'comment ' * 100 + '\n') * 100,
        'strings': ('x = "' + 'literal ' * 100 + '"\n') * 100,
        'fstrings': ('x = f"' + 'literal ' * 100 + '{name!r:>10}"\n') * 100,
        'spaces': ('x' + ' ' * 1000 + '= 1\n') * 100,
    })
    return sources


def measure(lexer, checker, source):
    token_count, ns = subprocess.check_output([str(lexer.resolve())], input=source.encode()).decode().strip().split(',')
    check = subprocess.check_output([str(checker.resolve()), '--reuse'], input=source.encode()).decode().strip().split(',')[-1]
    return {'tokens': int(token_count), 'lexer_ns': float(ns), 'check_ns': float(check)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lexer', type=Path, required=True)
    parser.add_argument('--checker', type=Path, required=True)
    parser.add_argument('--baseline-lexer', type=Path, required=True)
    parser.add_argument('--baseline-checker', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    results = []
    for name, source in workloads().items():
        before = []
        after = []
        # Reverse the order on the second round to reduce thermal/order bias.
        for order in [False, True]:
            pairs = [(args.baseline_lexer, args.baseline_checker, before), (args.lexer, args.checker, after)]
            for lexer, checker, samples in reversed(pairs) if order else pairs:
                samples.append(measure(lexer, checker, source))
        assert all(row['tokens'] == before[0]['tokens'] for row in before + after)
        row = {'name': name, 'bytes': len(source.encode()), 'tokens': before[0]['tokens'],
               'baseline_samples': before, 'optimized_samples': after}
        for key in ['lexer_ns', 'check_ns']:
            row[key + '_ratio'] = statistics.median(sample[key] for sample in after) / statistics.median(sample[key] for sample in before)
        results.append(row)
        print(f"{name}: lexer {row['lexer_ns_ratio']:.3f}×, check {row['check_ns_ratio']:.3f}×", flush=True)
    args.output.write_text(json.dumps({'platform': platform.platform(), 'python': sys.version, 'results': results}, indent=2) + '\n')


if __name__ == '__main__':
    main()
