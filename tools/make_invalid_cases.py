"""Mutate grammatical sources to exercise parser error recovery."""

import argparse
import json
import subprocess
from pathlib import Path

FAMILIES = [
    'brackets', 'format-spec', 'fstring-chain', 'class-pattern', 'mapping-pattern',
    'nested-class', 'mixed', 'power', 'lambda', 'postfix-mix', 'annotation',
]


def mutations(source):
    body = source.rstrip('\n')
    middle = len(body) // 2
    yield 'truncate-half', body[:middle]
    yield 'truncate-last', body[:-1]
    yield 'delete-middle', body[:middle] + body[middle + 1:]
    yield 'quote-middle', body[:middle] + "'" + body[middle:]
    yield 'closer-middle', body[:middle] + ']' + body[middle:]
    yield 'opener-middle', body[:middle] + '(' + body[middle:]
    yield 'extra-closer', body + ')'
    yield 'extra-quote', body + "'"
    yield 'extra-colon', body + ':'
    yield 'line-continuation', body + '\\'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spam-probe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--depths', type=int, nargs='+', default=[64, 256, 3000])
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    probe = args.spam_probe.resolve()
    manifest = {'cases': []}
    for family in FAMILIES:
        for depth in args.depths:
            source = subprocess.check_output([str(probe), 'generate', family, str(depth), '42'], timeout=5).decode()
            for mutation, altered in mutations(source):
                if not altered or altered == source:
                    continue
                name = f'{family}-{depth}-{mutation}'
                filename = f'{name}.py'
                (args.output / filename).write_text(altered)
                manifest['cases'].append({
                    'name': name, 'source': filename, 'family': f'invalid-{family}-{mutation}', 'depth': depth,
                })
    (args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'wrote {len(manifest["cases"])} mutations')


if __name__ == '__main__':
    main()
