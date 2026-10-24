"""Minimize a retained CPython parser failure with delta debugging."""

import argparse
import hashlib
import sqlite3
import subprocess
from pathlib import Path


PROBE = r"""
import ast, sys
try:
    ast.parse(sys.stdin.buffer.read(), filename="<spam-minimize>")
    print("ok")
except (MemoryError, RecursionError) as error:
    print(type(error).__name__)
except SyntaxError:
    print("syntax-error")
"""


def status(python, source, timeout):
    try:
        result = subprocess.run(
            [python, '-I', '-c', PROBE], input=source, capture_output=True,
            timeout=timeout, check=False,
        )
    except subprocess.TimeoutExpired:
        return 'timeout'
    if result.returncode != 0:
        return 'crash'
    return result.stdout.decode(errors='replace').strip()


def ddmin(source, interesting):
    granularity = 2
    while len(source) >= 2:
        chunk_size = (len(source) + granularity - 1) // granularity
        reduced = False
        for start in range(0, len(source), chunk_size):
            candidate = source[:start] + source[start + chunk_size:]
            if candidate and interesting(candidate):
                source = candidate
                granularity = max(2, granularity - 1)
                reduced = True
                print(f'{len(source)} bytes', flush=True)
                break
        if not reduced:
            if granularity >= len(source):
                break
            granularity = min(len(source), granularity * 2)
    return source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('case_id', help='full case id or an unambiguous prefix')
    parser.add_argument('--database', type=Path, default=Path('corpus/cases.sqlite'))
    parser.add_argument('--python', default='python3')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=3)
    parser.add_argument('--status', default='MemoryError')
    args = parser.parse_args()

    uri = args.database.resolve().as_uri() + '?mode=ro'
    with sqlite3.connect(uri, uri=True) as database:
        rows = database.execute(
            'SELECT id, source FROM cases WHERE id LIKE ?', (args.case_id + '%',)
        ).fetchall()
    if len(rows) != 1:
        parser.error(f'case prefix matched {len(rows)} rows')
    case_id, source = rows[0]
    if hashlib.sha256(source).hexdigest() != case_id:
        raise ValueError('case database digest does not match source')

    cache = {}

    def interesting(candidate):
        digest = hashlib.sha256(candidate).digest()
        if digest not in cache:
            cache[digest] = status(args.python, candidate, args.timeout) == args.status
        return cache[digest]

    if not interesting(source):
        parser.error(f'original source does not reproduce {args.status}')
    minimized = ddmin(source, interesting)
    observed = [status(args.python, minimized, args.timeout) for _ in range(5)]
    if observed != [args.status] * 5:
        raise RuntimeError(f'minimized result was unstable: {observed}')
    args.output.write_bytes(minimized)
    print(f'wrote {len(minimized)} bytes to {args.output}; {len(cache)} candidates tested')


if __name__ == '__main__':
    main()
