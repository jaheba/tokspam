"""Generate focused attempts to bypass Spam while stressing Python parsers."""

import argparse
import json
from pathlib import Path


def sources(depth):
    yield 'elif-chain', 'if False: pass\n' + 'elif False: pass\n' * depth
    yield 'missing-conditional-else', 'x = ' + '1 if ' * depth + '1\n'
    yield 'flat-fstring-fields', "x = f'" + '{x}' * depth + "'\n"
    yield 'flat-list-elements', 'x = [' + '0,' * depth + ']\n'
    yield 'flat-dict-elements', 'x = {' + "'x':0," * depth + '}\n'
    yield 'flat-comprehension-ifs', 'x = [0 for x in (0,) ' + 'if x ' * depth + ']\n'
    yield 'flat-match-cases', 'match x:\n' + ' case 0: pass\n' * depth
    yield 'unicode-normalization', 'ℌ' * depth + ' = 1\n'
    yield 'adjacent-strings', 'x = ' + "'x' " * depth + '\n'
    yield 'invalid-if-tokens', 'x = 1 ' + 'if ' * depth + '\n'
    yield 'invalid-pattern-as', 'match x:\n case ' + '1 as ' * depth + 'x: pass\n'
    yield 'alternating-bool', 'x = ' + '1 or 1 and ' * depth + '1\n'
    yield 'flat-call-args', 'x = f(' + '0,' * depth + ')\n'
    yield 'flat-function-params', 'def f(' + ','.join(f'x{i}' for i in range(depth)) + '): pass\n'
    yield 'flat-slice-items', 'x = a[' + '0,' * depth + ']\n'
    yield 'flat-sequence-pattern', 'match x:\n case [' + '0,' * depth + ']: pass\n'
    yield 'flat-except-handlers', 'try: pass\n' + 'except E: pass\n' * depth
    yield 'long-hex-integer', 'x = 0x' + 'f' * depth + '\n'
    yield 'long-float-fraction', 'x = 0.' + '1' * depth + '\n'
    yield 'single-string-escapes', "x = '" + '\\x41' * depth + "'\n"
    yield 'single-unicode-name-escapes', "x = '" + '\\N{LATIN CAPITAL LETTER A}' * depth + "'\n"
    yield 'combining-identifier', 'a' + '\N{COMBINING GRAVE ACCENT}' * depth + ' = 1\n'
    yield 'invalid-long-number', 'x = ' + '1_' * depth + '\n'
    yield 'invalid-walrus-chain', 'x = ' + 'a := ' * depth + '1\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--depths', type=int, nargs='+', default=[1_000, 10_000, 50_000])
    parser.add_argument('--max-bytes', type=int, default=1024 * 1024)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = {'cases': []}
    for depth in args.depths:
        if not 1 <= depth <= 350_000:
            parser.error('depths must be 1..350000')
        for family, source in sources(depth):
            encoded = source.encode()
            if len(encoded) > args.max_bytes:
                continue
            name = f'{family}-{depth}'
            filename = f'{name}.py'
            (args.output / filename).write_bytes(encoded)
            manifest['cases'].append({
                'name': name, 'source': filename, 'family': f'attack-{family}', 'depth': depth,
            })
    (args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'wrote {len(manifest["cases"])} cases')


if __name__ == '__main__':
    main()
