"""Find Spam rejections in the current Python interpreter's standard library."""

import argparse
import ast
import json
import subprocess
import sysconfig
from pathlib import Path


def probe(command, source, timeout):
    try:
        result = subprocess.run(command, input=source, capture_output=True, timeout=timeout, check=False)
    except subprocess.TimeoutExpired:
        return {'status': 'timeout'}
    return {
        'status': result.stdout.decode('utf-8', errors='replace').strip(),
        'returncode': result.returncode,
        'stderr': result.stderr.decode('utf-8', errors='replace')[-1000:],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spam-probe', type=Path, required=True)
    parser.add_argument('--ruff-probe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=2.0)
    parser.add_argument('--stdlib', type=Path, default=Path(sysconfig.get_path('stdlib')))
    args = parser.parse_args()
    spam = args.spam_probe.resolve()
    ruff = args.ruff_probe.resolve()
    counts = {'files': 0, 'valid_utf8_parse': 0, 'oversize': 0, 'spam_rejections': 0, 'ruff_parsed_rejections': 0}
    rejections = []

    for path in sorted(args.stdlib.rglob('*.py')):
        if 'site-packages' in path.parts or '__pycache__' in path.parts:
            continue
        counts['files'] += 1
        source = path.read_bytes()
        if len(source) > 1024 * 1024:
            counts['oversize'] += 1
            continue
        try:
            ast.parse(source.decode('utf-8'), filename=str(path))
        except (UnicodeDecodeError, SyntaxError):
            continue
        counts['valid_utf8_parse'] += 1
        screened = probe([str(spam), 'check', '100'], source, args.timeout)
        if not screened['status'].startswith('reject:'):
            continue
        parsed = probe([str(ruff)], source, args.timeout)
        counts['spam_rejections'] += 1
        counts['ruff_parsed_rejections'] += parsed['status'] == 'ok' and parsed['returncode'] == 0
        rejections.append({
            'path': str(path.relative_to(args.stdlib)),
            'bytes': len(source),
            'spam': screened['status'],
            'ruff': parsed['status'],
        })

    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'summary.json').write_text(json.dumps(counts, indent=2) + '\n')
    with (args.output / 'rejections.jsonl').open('w') as output:
        for rejection in rejections:
            output.write(json.dumps(rejection) + '\n')
    print(json.dumps(counts, indent=2))


if __name__ == '__main__':
    main()
