"""Combine class-body nesting with long left-associated expressions."""

import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--class-depths', type=int, nargs='+', default=[0, 20, 80])
    parser.add_argument('--chain-lengths', type=int, nargs='+', default=[30_000, 50_000, 70_000])
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = {'cases': []}
    for classes in args.class_depths:
        for chain in args.chain_lengths:
            if not 0 <= classes <= 90 or not 1 <= chain <= 100_000:
                parser.error('classes must be 0..90 and chain lengths 1..100000')
            source = ''.join(' ' * depth + f'class C{depth}:\n' for depth in range(classes))
            source += ' ' * classes + 'x = ' + '1 + ' * chain + '1\n'
            name = f'classes-{classes}-chain-{chain}'
            filename = f'{name}.py'
            (args.output / filename).write_text(source)
            manifest['cases'].append({'name': name, 'source': filename,
                                      'family': f'class-chain-{classes}', 'depth': chain})
    (args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'wrote {len(manifest["cases"])} cases')


if __name__ == '__main__':
    main()
