"""Grammar-directed parser fuzzing. Generated programs are parsed, never executed."""

import argparse
import hashlib
import json
import os
import random
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

CPYTHON_PROBE = r"""
import ast, sys
source = sys.stdin.buffer.read(1024 * 1024 + 1)
if len(source) > 1024 * 1024:
    print("source-limit")
    raise SystemExit
try:
    tree = ast.parse(source, filename="<spam-fuzz>")
    if sys.argv[1] == "compile":
        compile(tree, "<spam-fuzz>", "exec")
    print("ok")
except SyntaxError as error:
    text = str(error).lower()
    limits = ("too many nested", "too deeply nested", "nested too deeply", "too much nesting", "maximum number")
    print("syntax-limit" if any(message in text for message in limits) else "syntax-error")
except (RecursionError, MemoryError) as error:
    print(type(error).__name__)
"""


def child_limits(memory_mib):
    """Linux address-space limit; Unix core dumps are disabled on other hosts too."""
    if os.name != 'posix':
        return None

    def apply():
        import resource

        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        if sys.platform == 'linux':
            ceiling = memory_mib * 1024 * 1024
            resource.setrlimit(resource.RLIMIT_AS, (ceiling, ceiling))

    return apply


def run(command, source, timeout, memory_mib):
    start = time.monotonic()
    try:
        result = subprocess.run(
            command,
            input=source,
            capture_output=True,
            timeout=timeout,
            preexec_fn=child_limits(memory_mib),
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {'status': 'timeout', 'seconds': time.monotonic() - start}
    status = result.stdout.decode('utf-8', errors='replace').strip()
    if result.returncode != 0:
        status = 'crash' if result.returncode < 0 else 'error'
    return {
        'status': status,
        'returncode': result.returncode,
        'seconds': time.monotonic() - start,
        'stderr': result.stderr.decode('utf-8', errors='replace')[-1000:],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spam-probe', type=Path, required=True)
    parser.add_argument('--spam-check', choices=['check', 'check-widths'], default='check')
    parser.add_argument('--ruff-probe', type=Path, required=True)
    parser.add_argument('--monty-probe', type=Path)
    parser.add_argument('--monty-mode', choices=['compile', 'run'], default='compile')
    parser.add_argument('--issue-repro', action='store_true', help='also run the dynamic eval reproducer from Monty issue 1042')
    parser.add_argument('--manifest', type=Path, help='run the checked-in source and generated cases listed in a JSON manifest')
    parser.add_argument('--database', type=Path, help='replay all retained sources from a SQLite case database')
    parser.add_argument('--python', default=sys.executable)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--seed', type=int, default=42)
    parser.add_argument('--random-cases', type=int, default=100)
    parser.add_argument('--max-depth', type=int, default=3000)
    parser.add_argument('--screen-depth', type=int, default=100)
    parser.add_argument('--timeout', type=float, default=2.0)
    parser.add_argument('--memory-mib', type=int, default=512)
    parser.add_argument('--phase', choices=['parse', 'compile'], default='parse')
    args = parser.parse_args()
    if not 1 <= args.max_depth <= 100_000 or args.random_cases < 0 or args.timeout <= 0:
        parser.error('depth must be 1..100000, case count nonnegative, and timeout positive')
    if args.issue_repro and (not args.monty_probe or args.monty_mode != 'run'):
        parser.error('--issue-repro requires --monty-probe and --monty-mode run')
    if sum((bool(args.manifest), bool(args.database), args.issue_repro)) > 1:
        parser.error('--manifest, --database, and --issue-repro cannot be combined')
    args.spam_probe = args.spam_probe.resolve()
    args.ruff_probe = args.ruff_probe.resolve()
    if args.monty_probe:
        args.monty_probe = args.monty_probe.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    families = subprocess.check_output([str(args.spam_probe), 'families'], text=True, timeout=5).splitlines()
    rng = random.Random(args.seed)

    def generate(family, depth, seed):
        if family == 'runtime-eval-binary':
            return f"eval('1+' * {depth} + '1')\n".encode()
        return subprocess.check_output([str(args.spam_probe), 'generate', family, str(depth), str(seed)], timeout=5)

    def probe(source):
        result = {
            'cpython': run([args.python, '-I', '-c', CPYTHON_PROBE, args.phase], source, args.timeout, args.memory_mib),
            'ruff': run([str(args.ruff_probe)], source, args.timeout, args.memory_mib),
            'spam': run(
                [str(args.spam_probe), args.spam_check, str(args.screen_depth)],
                source,
                args.timeout,
                args.memory_mib,
            ),
        }
        if args.monty_probe:
            command = [str(args.monty_probe)]
            if args.monty_mode == 'run':
                command.append('--run')
            result['monty'] = run(command, source, args.timeout, args.memory_mib)
        return result

    if args.database:
        with sqlite3.connect(args.database) as db:
            cases = []
            for case_id, family, depth, seed, source in db.execute(
                'SELECT id, family, depth, seed, source FROM cases ORDER BY family, id'
            ):
                if hashlib.sha256(source).hexdigest() != case_id:
                    raise ValueError(f'corrupt case: {case_id}')
                cases.append((case_id[:12], family, depth, int(seed) if seed and seed != 'None' else None, source))
    elif args.manifest:
        manifest = json.loads(args.manifest.read_text())
        cases = []
        names = set()
        for case in manifest['cases']:
            name = case['name']
            if name in names:
                raise ValueError(f'duplicate manifest case: {name}')
            names.add(name)
            if 'source' in case:
                source = (args.manifest.parent / case['source']).read_bytes()
                family = case.get('family', 'source')
                depth = case.get('depth')
                seed = case.get('seed')
            else:
                family, depth, seed = case['family'], case['depth'], case.get('seed', args.seed)
                if family not in families or not 1 <= depth <= 100_000:
                    raise ValueError(f'invalid generated case: {name}')
                source = generate(family, depth, seed)
            cases.append((name, family, depth, seed, source))
    else:
        # Verify each construction before amplifying its depth.
        for family in families:
            result = probe(generate(family, 1, args.seed))
            if any(result[engine]['status'] != 'ok' for engine in ('cpython', 'ruff')):
                raise RuntimeError(f'invalid shallow family {family}: {result}')

        boundaries = sorted({1, 8, 32, 80, 100, 101, 200, 256, args.max_depth})
        generated = [(family, depth, args.seed) for family in families for depth in boundaries if depth <= args.max_depth]
        generated += [
            (rng.choice(families), rng.randint(1, args.max_depth), rng.getrandbits(64)) for _ in range(args.random_cases)
        ]
        if args.issue_repro:
            generated.append(('runtime-eval-binary', 1_000_000, args.seed))
        cases = [(f'{family}-{depth}-{index}', family, depth, seed, generate(family, depth, seed))
                 for index, (family, depth, seed) in enumerate(generated)]
    counts = {'cases': 0, 'parser_failures': 0, 'spam_bypasses': 0,
              'spam_rejections_of_parsed_source': 0, 'syntax_limits': 0, 'syntax_errors': 0}
    failures = {'crash', 'error', 'timeout', 'RecursionError', 'MemoryError'}
    with (args.output / 'results.jsonl').open('w') as output:
        for name, family, depth, seed, source in cases:
            if family == 'mixed':
                shallow = probe(generate(family, min(depth, 4), seed))
                if any(shallow[engine]['status'] != 'ok' for engine in ('cpython', 'ruff')):
                    raise RuntimeError(f'invalid mixed seed {seed}: {shallow}')
            result = probe(source)
            record = {'name': name, 'family': family, 'depth': depth, 'seed': seed,
                      'case_id': hashlib.sha256(source).hexdigest(), 'bytes': len(source), **result}
            counts['cases'] += 1
            engines = ('cpython', 'ruff', 'monty') if args.monty_probe else ('cpython', 'ruff')
            broken = any(result[engine]['status'] in failures for engine in engines)
            counts['syntax_limits'] += any(result[e]['status'] == 'syntax-limit' for e in ('cpython', 'ruff'))
            counts['syntax_errors'] += any(result[e]['status'] == 'syntax-error' for e in ('cpython', 'ruff'))
            if result['spam']['status'] not in {'accept'} and not result['spam']['status'].startswith('reject:'):
                raise RuntimeError(f'Spam probe failed: {record}')
            parsed_both = all(result[engine]['status'] == 'ok' for engine in ('cpython', 'ruff'))
            conservative_rejection = parsed_both and result['spam']['status'].startswith('reject:')
            counts['spam_rejections_of_parsed_source'] += conservative_rejection
            if broken:
                counts['parser_failures'] += 1
                bypass = result['spam']['status'] == 'accept'
                counts['spam_bypasses'] += bypass
                digest = hashlib.sha256(source).hexdigest()[:16]
                path = args.output / f'{name}-{digest}.py'
                path.write_bytes(source)
                record['artifact'] = path.name
                details = ' '.join(f'{engine}={result[engine]["status"]}' for engine in engines)
                print(f'{"BYPASS" if bypass else "BLOCKED"} {name}: {details}', flush=True)
            elif conservative_rejection:
                digest = hashlib.sha256(source).hexdigest()[:16]
                path = args.output / f'{name}-{digest}.py'
                path.write_bytes(source)
                record['artifact'] = path.name
                print(f'REJECTED-PARSED {name}: {result["spam"]["status"]}', flush=True)
            output.write(json.dumps(record) + '\n')
            output.flush()
    summary = {
        **counts,
        'seed': args.seed,
        'phase': args.phase,
        'monty_mode': args.monty_mode if args.monty_probe else None,
        'python_version': subprocess.check_output([args.python, '--version'], text=True).strip(),
        'screen_depth': args.screen_depth,
        'timeout_seconds': args.timeout,
        'memory_limit_mib': args.memory_mib if sys.platform == 'linux' else None,
        'platform': sys.platform,
        'manifest': str(args.manifest) if args.manifest else None,
        'database': str(args.database) if args.database else None,
        'ruff_probe_sha256': hashlib.sha256(args.ruff_probe.read_bytes()).hexdigest(),
        'spam_probe_sha256': hashlib.sha256(args.spam_probe.read_bytes()).hexdigest(),
        'monty_probe_sha256': hashlib.sha256(args.monty_probe.read_bytes()).hexdigest() if args.monty_probe else None,
    }
    (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))
    return int(counts['spam_bypasses'] > 0)


if __name__ == '__main__':
    raise SystemExit(main())
