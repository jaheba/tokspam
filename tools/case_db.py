"""Import parser campaigns and resource observations into SQLite."""

import argparse
import hashlib
import importlib.util
import json
import sqlite3
from pathlib import Path

FAILURES = {'crash', 'error', 'timeout', 'RecursionError', 'MemoryError'}
SCHEMA = Path(__file__).resolve().parent.parent / 'corpus' / 'schema.sql'


def connect(path, readonly=False):
    db = sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) if readonly else sqlite3.connect(path)
    if not readonly:
        db.executescript(SCHEMA.read_text())
        db.execute('''
            INSERT OR IGNORE INTO case_labels (case_id, label, first_seen_run)
            SELECT c.id, 'parser-failure', c.first_seen_run FROM cases c
            WHERE NOT EXISTS (SELECT 1 FROM case_labels l WHERE l.case_id = c.id)
        ''')
    return db


def import_campaign(db, campaign):
    metadata = json.loads((campaign / 'summary.json').read_text())
    records = [json.loads(line) for line in (campaign / 'results.jsonl').read_text().splitlines()]
    selected = {}
    existing = {row[0] for row in db.execute('SELECT id FROM cases')}
    repeated = []
    for record in records:
        failures = {engine: record[engine]['status'] for engine in ('cpython', 'ruff')
                    if record[engine]['status'] in FAILURES}
        parsed_both = all(record[engine]['status'] == 'ok' for engine in ('cpython', 'ruff'))
        rejected = record['spam']['status'].startswith('reject:')
        label = 'parser-failure' if failures else 'conservative-rejection' if parsed_both and rejected else None
        if record.get('case_id') in existing:
            repeated.append((record, None, label))
            continue
        if 'artifact' not in record or label is None:
            continue
        source = (campaign / record['artifact']).read_bytes()
        if hashlib.sha256(source).hexdigest() in existing:
            repeated.append((record, source, label))
            continue
        reason = record['spam']['status'].split(':', 2)[1] if rejected else None
        key = (record['family'], label, tuple(sorted(failures.items())) if failures else reason)
        if key not in selected or len(source) < len(selected[key][1]):
            selected[key] = (record, source, label)

    with db:
        run_id = db.execute('INSERT INTO runs (metadata) VALUES (?)', (json.dumps(metadata),)).lastrowid
        for record, source, label in [*selected.values(), *repeated]:
            case_id = record.get('case_id') or hashlib.sha256(source).hexdigest()
            if source is not None:
                db.execute(
                    'INSERT OR IGNORE INTO cases (id, source, family, depth, seed, first_seen_run) '
                    'VALUES (?, ?, ?, ?, ?, ?)',
                    (case_id, source, record['family'], record['depth'],
                     str(record['seed']) if record['seed'] is not None else None, run_id),
                )
            if label is not None:
                db.execute('INSERT OR IGNORE INTO case_labels (case_id, label, first_seen_run) VALUES (?, ?, ?)',
                           (case_id, label, run_id))
            for implementation in ('cpython', 'ruff', 'spam', 'monty'):
                if implementation not in record:
                    continue
                result = record[implementation]
                db.execute(
                    'INSERT OR REPLACE INTO observations '
                    '(run_id, case_id, implementation, status, seconds, returncode, stderr) '
                    'VALUES (?, ?, ?, ?, ?, ?, ?)',
                    (run_id, case_id, implementation, result['status'], result.get('seconds'),
                     result.get('returncode'), result.get('stderr')),
                )
    print(f'imported {len(selected)} new shapes and {len(repeated)} known cases into run {run_id}')


def import_resource_campaign(db, campaign):
    metadata = json.loads((campaign / 'resource-summary.json').read_text())
    records = [json.loads(line) for line in (campaign / 'resource-results.jsonl').read_text().splitlines()]
    generator_module = None
    if generator_path := metadata.get('generator'):
        spec = importlib.util.spec_from_file_location('spam_resource_generator', campaign / generator_path)
        if spec is None or spec.loader is None:
            raise ValueError(f'cannot load resource generator: {generator_path}')
        generator_module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator_module)

    with db:
        run_id = db.execute('INSERT INTO runs (metadata) VALUES (?)', (json.dumps(metadata),)).lastrowid
        imported = 0
        for record in records:
            if source_path := record.get('source'):
                source = (campaign / source_path).read_bytes()
            elif generator_module is not None and record.get('generator'):
                generated = generator_module.generate(
                    record['generator']['name'], record['generator'].get('parameters', {})
                )
                source = generated.encode() if isinstance(generated, str) else generated
            else:
                raise ValueError(f'resource case has no source or generator: {record.get("name", "unknown")}')
            case_id = hashlib.sha256(source).hexdigest()
            if record.get('case_id') not in (None, case_id):
                raise ValueError(f'case digest does not match source: {record.get("name", case_id)}')

            db.execute(
                'INSERT OR IGNORE INTO cases (id, source, family, depth, seed, first_seen_run) '
                'VALUES (?, ?, ?, ?, ?, ?)',
                (case_id, source, record['family'], record.get('depth'),
                 str(record['seed']) if record.get('seed') is not None else None, run_id),
            )
            generator = record.get('generator')
            if generator:
                db.execute(
                    'INSERT OR REPLACE INTO case_generators (case_id, generator, parameters) VALUES (?, ?, ?)',
                    (case_id, generator['name'], json.dumps(generator.get('parameters', {}), sort_keys=True)),
                )
            for label in record.get('labels', []):
                db.execute(
                    'INSERT OR IGNORE INTO case_labels (case_id, label, first_seen_run) VALUES (?, ?, ?)',
                    (case_id, label, run_id),
                )

            for observation in record['observations']:
                samples = observation.get('samples_seconds', [])
                db.execute(
                    'INSERT OR REPLACE INTO resource_observations '
                    '(run_id, case_id, implementation, phase, outcome, status, sample_count, '
                    'median_seconds, min_seconds, max_seconds, peak_rss_bytes, baseline_rss_bytes, '
                    'time_limit_seconds, memory_limit_bytes, samples, details) '
                    'VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
                    (run_id, case_id, observation['implementation'], observation['phase'], observation['outcome'],
                     observation['status'], len(samples), observation.get('median_seconds'),
                     min(samples) if samples else None, max(samples) if samples else None,
                     observation.get('peak_rss_bytes'), observation.get('baseline_rss_bytes'),
                     observation.get('time_limit_seconds'), observation.get('memory_limit_bytes'),
                     json.dumps(samples), observation.get('details')),
                )
                db.execute(
                    'INSERT OR REPLACE INTO observations '
                    '(run_id, case_id, implementation, status, seconds, returncode, stderr) '
                    'VALUES (?, ?, ?, ?, ?, ?, ?)',
                    (run_id, case_id, observation['implementation'], observation['status'],
                     observation.get('median_seconds'), observation.get('returncode'), observation.get('stderr')),
                )
            imported += 1
    print(f'imported {imported} resource cases into run {run_id}')


def list_cases(db):
    for case_id, family, size, labels, observations in db.execute('''
        SELECT c.id, c.family, length(c.source),
               (SELECT group_concat(label, ',') FROM case_labels l WHERE l.case_id = c.id),
               group_concat(o.implementation || '=' || o.status, ', ')
        FROM cases c
        LEFT JOIN observations o ON o.case_id = c.id AND o.run_id = (
            SELECT max(o2.run_id) FROM observations o2 WHERE o2.case_id = c.id
        )
        GROUP BY c.id ORDER BY c.family, c.id
    '''):
        print(f'{case_id[:12]}  {family:<24} {size:>7} bytes  {labels}: {observations}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['import', 'import-resource', 'list'])
    parser.add_argument('campaign', type=Path, nargs='?')
    parser.add_argument('--database', type=Path, default=Path('corpus/cases.sqlite'))
    args = parser.parse_args()
    if args.action in {'import', 'import-resource'} and args.campaign is None:
        parser.error(f'{args.action} requires a campaign directory')
    if args.action in {'import', 'import-resource'}:
        args.database.parent.mkdir(parents=True, exist_ok=True)
    with connect(args.database.resolve(), readonly=args.action == 'list') as db:
        if args.action == 'import':
            import_campaign(db, args.campaign)
        elif args.action == 'import-resource':
            import_resource_campaign(db, args.campaign)
        else:
            list_cases(db)


if __name__ == '__main__':
    main()
