"""Write a manifest that exercises every Python expression operator family."""

import argparse
import json
from pathlib import Path

BINARY = {
    'add': '+', 'subtract': '-', 'multiply': '*', 'matrix-multiply': '@',
    'divide': '/', 'floor-divide': '//', 'modulo': '%', 'power': '**',
    'left-shift': '<<', 'right-shift': '>>', 'bit-and': '&', 'bit-xor': '^', 'bit-or': '|',
    'bool-and': 'and', 'bool-or': 'or',
    'equal': '==', 'not-equal': '!=', 'less': '<', 'less-equal': '<=',
    'greater': '>', 'greater-equal': '>=', 'is': 'is', 'is-not': 'is not',
    'in': 'in', 'not-in': 'not in',
}
UNARY = {'positive': '+', 'negative': '-', 'invert': '~', 'not': 'not '}
AUGMENTED = ['+=', '-=', '*=', '@=', '/=', '//=', '%=', '**=', '<<=', '>>=', '&=', '^=', '|=']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--depths', type=int, nargs='+', default=[80, 256, 1024])
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    cases = []
    for depth in args.depths:
        if not 1 <= depth <= 10_000:
            parser.error('depths must be 1..10000')
        for name, operator in BINARY.items():
            source = 'x = ' + (f'1 {operator} ' * depth) + '1\n'
            cases.append((f'{name}-{depth}', source))
        for name, operator in UNARY.items():
            cases.append((f'{name}-{depth}', 'x = ' + operator * depth + '1\n'))
        for operator in AUGMENTED:
            name = 'aug-' + operator.replace('=', '').replace('/', 'slash').replace('*', 'star')
            cases.append((f'{name}-{depth}', 'x = 1\n' + (f'x {operator} 1\n' * depth)))
        cases.append((f'walrus-{depth}', 'x = ' + ('(a := ' * depth) + '1' + (')' * depth) + '\n'))

    manifest = {'cases': []}
    for name, source in cases:
        filename = f'{name}.py'
        (args.output / filename).write_text(source)
        manifest['cases'].append({'name': name, 'source': filename, 'family': name.rsplit('-', 1)[0],
                                  'depth': int(name.rsplit('-', 1)[1])})
    (args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'wrote {len(cases)} cases')


if __name__ == '__main__':
    main()
