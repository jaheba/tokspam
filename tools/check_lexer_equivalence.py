"""Compare exact lexer token streams on malformed inputs, stdlib, and corpus."""

import argparse
import json
import random
import sqlite3
import subprocess
import sysconfig
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--lexer', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    rng = random.Random(20261024)
    alphabet = list("a1 \t\f\n\r'\"ft{}[]()\\:!🦀é中")
    sources = [''.join(rng.choice(alphabet) for _ in range(rng.randrange(500)))
               for _ in range(1000)]
    stdlib = [path for path in Path(sysconfig.get_path('stdlib')).rglob('*.py')
              if 'site-packages' not in path.parts and '__pycache__' not in path.parts]
    sources.extend(path.read_text(errors='replace') for path in stdlib)

    # Open the corpus read-only so the comparison cannot change campaign data.
    with sqlite3.connect('file:corpus/cases.sqlite?mode=ro', uri=True) as database:
        for (source,) in database.execute('select source from cases'):
            sources.append(source if isinstance(source, str) else bytes(source).decode())

    for index, source in enumerate(sources):
        outputs = [subprocess.check_output([str(binary.resolve()), '--tokens'],
                                          input=source.encode())
                   for binary in [args.baseline, args.lexer]]
        if outputs[0] != outputs[1]:
            raise RuntimeError(f'Token mismatch in case {index}: {source[:100]!r}')

    report = {'seed': 20261024, 'random_cases': 1000, 'stdlib_files': len(stdlib),
              'total_cases': len(sources), 'mismatches': 0,
              'compared': 'token kinds, source ranges, and string contents ranges'}
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(report)


if __name__ == '__main__':
    main()
