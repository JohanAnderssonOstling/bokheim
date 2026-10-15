#!/usr/bin/env python3
"""Resume a subject-only catalog backfill using verified metadata classifications."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
import logging
from pathlib import Path
import shutil
import sqlite3
import subprocess
import time
from urllib.request import Request, urlopen

SUPPORTED_POLICIES = frozenset(('books_v7', 'books_v8', 'books_v9', 'books_v10', 'books_v11', 'books_v12', 'books_v13', 'books_v19', 'books_v20', 'books_v21', 'books_v22', 'books_v23', 'books_v24', 'books_v25', 'books_v26', 'books_v27'))


def select_entries(audit, missing_subjects=False):
    """Missing navigation is eligible even without an existing LCC code."""
    return [row for row in audit if not row['projected_paths'] or (not missing_subjects and row['needs_refinement'])]


def now():
    return datetime.now(timezone.utc).isoformat(timespec='seconds')


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


def apply_subjects(db, target, result, supported_lcc):
    if target['table'] not in ('book', 'external_book'):
        raise ValueError('Unknown catalog record table')
    if result['status'] == 'failed':
        raise RuntimeError(result.get('detail', 'Metadata classification failed'))
    record = result.get('record')
    if not record:
        return []
    candidates = [subject for subject in record['book']['subjects'] if subject.get('code') and (
        subject.get('authority') in ('lcc', 'ddc', 'bisac') and (subject['authority'] != 'lcc' or subject['code'] in supported_lcc)
        or subject.get('authority') == 'unified' and subject.get('source', '').startswith('metadata:source-heading:')
    )]
    table = target['table']
    key = 'hash' if table == 'book' else 'entry_key'
    db.execute('BEGIN IMMEDIATE')
    try:
        row = db.execute(f'SELECT record FROM {table} WHERE {key}=?', (target['key'],)).fetchone()
        if not row:
            raise ValueError('The catalog entry was removed during enrichment')
        latest = json.loads(row[0])
        subjects = latest['book']['subjects']
        additions = []
        for subject in candidates:
            if subject not in subjects:
                subjects.append(subject)
                additions.append(subject)
        if additions:
            db.execute(f'UPDATE {table} SET record=? WHERE {key}=?', (json.dumps(latest, ensure_ascii=False), target['key']))
        db.commit()
        return additions
    except Exception:
        db.rollback()
        raise


def fetch_batch(args, targets):
    books = []
    with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db:
        for target in targets:
            table = target['table']
            if table not in ('book', 'external_book'):
                raise ValueError('Unknown catalog record table')
            key = 'hash' if table == 'book' else 'entry_key'
            row = db.execute(f'SELECT record FROM {table} WHERE {key}=?', (target['key'],)).fetchone()
            if not row:
                raise ValueError('Selected catalog entry is missing')
            record = json.loads(row[0])
            record['description'] = ''
            books.append({'key': table + ':' + target['key'], 'record': record, 'audiobook': False, 'needs_cover': False, 'readings': [], 'recording': None})
    body = json.dumps({'books': books}, ensure_ascii=False).encode()
    for attempt in range(3):
        try:
            with urlopen(Request(args.metadata_url.rstrip('/') + '/v3/books', data=body, headers={'Content-Type': 'application/json', 'User-Agent': 'bokheim-catalog'}), timeout=180) as response:
                result = json.load(response)
            if result.get('policy') not in SUPPORTED_POLICIES:
                raise ValueError('Unsupported metadata classification policy: ' + str(result.get('policy')))
            results = {item['key']: item for item in result['results']}
            if len(result['results']) != len(books) or len(results) != len(books) or set(results) != {book['key'] for book in books}:
                raise ValueError('Metadata response omitted or duplicated a requested book')
            return {'results': results, 'request_sha256': hashlib.sha256(body).hexdigest(), 'policy': result['policy'], 'observed_at': now()}
        except Exception:
            if attempt == 2:
                raise
            time.sleep(2 ** (attempt + 1))


def publish(args):
    for attempt in range(5):
        root = args.catalog / 'published'
        releases = root / 'releases'
        release = max((int(path.name) for path in releases.iterdir() if path.name.isdigit()), default=0) + 1
        result = subprocess.run([str(args.binary), str(args.catalog), 'publish', str(root), str(release)], capture_output=True, text=True)
        if result.returncode:
            logging.warning('Publication retry: %s', result.stderr[-1000:])
            time.sleep(2)
            continue
        snapshot = releases / str(release) / 'catalog.sqlite3'
        with sqlite3.connect(f'file:{snapshot}?mode=ro', uri=True) as db:
            assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
            assert not db.execute('PRAGMA foreign_key_check').fetchall()
        installed = args.catalog.parent / 'catalog.sqlite3'
        temporary = installed.with_suffix('.sqlite3.subjects-partial')
        shutil.copyfile(snapshot, temporary)
        temporary.replace(installed)
        write_json(args.output / 'last-publication.json', {'release_id': release, 'published_at': now(), 'installed': str(installed)})
        return
    raise RuntimeError('Could not install a catalog snapshot after five publication attempts')


def run(args):
    args.output.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(level=logging.INFO, format='%(asctime)s %(levelname)s %(message)s', handlers=[logging.StreamHandler(), logging.FileHandler(args.output / 'backfill.log')])
    with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db:
        backup_path = args.output / 'curation-before.sqlite3'
        if not backup_path.exists():
            with sqlite3.connect(backup_path) as backup:
                db.backup(backup)
    selection_path = args.output / 'selection.json'
    scope = 'missing-subjects' if args.missing_subjects else 'unplaced-or-broad'
    if not selection_path.exists():
        result = subprocess.run([str(args.binary), str(args.catalog), 'subject-audit'], capture_output=True, text=True, check=True)
        audit = json.loads(result.stdout)
        write_json(args.output / 'before-audit.json', audit)
        write_json(selection_path, {'selected_at': now(), 'scope': scope, 'supported_policies': sorted(SUPPORTED_POLICIES), 'entries': select_entries(audit, args.missing_subjects)})
    selection = json.loads(selection_path.read_text())
    if args.missing_subjects and selection.get('scope') != scope:
        raise ValueError('Use a new output directory for a missing-subjects selection')
    progress = args.output / 'progress.json'
    state = json.loads(progress.read_text()) if progress.exists() else {'started_at': now(), 'selected': len(selection['entries']), 'outcomes': {}}
    pending = [row for row in selection['entries'] if state['outcomes'].get(row['table'] + ':' + row['key'], {}).get('state') not in ('updated', 'unchanged')]
    if args.limit:
        pending = pending[:args.limit]
    state['state'] = 'running'
    state.pop('finished_at', None)
    write_json(progress, state)
    logging.info('Refining %s pending entries from %s selected candidates', len(pending), state['selected'])
    batches = [pending[index:index + args.batch_size] for index in range(0, len(pending), args.batch_size)]
    processed_since_publish = 0
    resolver = subprocess.Popen([str(args.binary), str(args.catalog), 'resolve-subject-codes'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    with ThreadPoolExecutor(max_workers=args.workers) as pool, sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db, resolver:
        futures = [(targets, pool.submit(fetch_batch, args, targets)) for targets in batches]
        for targets, future in futures:
            try:
                response = future.result()
                codes = sorted({subject['code'] for result in response['results'].values() for subject in (result.get('record') or {}).get('book', {}).get('subjects', []) if subject.get('authority') == 'lcc' and subject.get('code')})
                resolver.stdin.write(json.dumps(codes) + '\n')
                resolver.stdin.flush()
                line = resolver.stdout.readline()
                if not line:
                    raise RuntimeError('The local classification validator stopped unexpectedly')
                supported_lcc = {row['code'] for row in json.loads(line) if row['specific']}
            except Exception as error:
                response = None
                batch_error = str(error)
            for target in targets:
                identity = target['table'] + ':' + target['key']
                try:
                    if response is None:
                        raise RuntimeError(batch_error)
                    result = response['results'][identity]
                    additions = apply_subjects(db, target, result, supported_lcc)
                    state['outcomes'][identity] = {'state': 'updated' if additions else 'unchanged', 'title': target['title'], 'added_subjects': additions, 'metadata_status': result['status'], 'policy': response['policy'], 'request_sha256': response['request_sha256'], 'completed_at': now()}
                    logging.info('%s: %s verified classification additions', target['title'], len(additions))
                except Exception as error:
                    state['outcomes'][identity] = {'state': 'failed', 'title': target['title'], 'error': str(error), 'updated_at': now()}
                    logging.warning('%s failed: %s', target['title'], error)
                for status in ('updated', 'unchanged', 'failed'):
                    state[status] = sum(value['state'] == status for value in state['outcomes'].values())
                state['processed'] = state['updated'] + state['unchanged']
                state['updated_at'] = now()
                write_json(progress, state)
                processed_since_publish += 1
            if processed_since_publish >= args.publish_every:
                publish(args)
                processed_since_publish = 0
        resolver.stdin.close()
    publish(args)
    result = subprocess.run([str(args.binary), str(args.catalog), 'subject-audit'], capture_output=True, text=True, check=True)
    after = json.loads(result.stdout)
    write_json(args.output / 'after-audit.json', after)
    keyed = {(row['table'], row['key']): row for row in after}
    surviving = [row for row in selection['entries'] if (row['table'], row['key']) in keyed]
    state['refined'] = sum(row['needs_refinement'] and not keyed[(row['table'], row['key'])]['needs_refinement'] for row in surviving)
    state['unplaced_before'] = sum(not row['projected_paths'] for row in selection['entries'])
    state['newly_placed'] = sum(not row['projected_paths'] and bool(keyed[(row['table'], row['key'])]['projected_paths']) for row in surviving)
    state['remaining_unplaced'] = sum(not keyed[(row['table'], row['key'])]['projected_paths'] for row in surviving)
    state['state'] = 'completed' if state['processed'] == state['selected'] else ('partial' if args.limit else 'completed_with_failures')
    state['finished_at'] = now()
    write_json(progress, state)
    logging.info('Finished: %s updated, %s unchanged, %s failed; %s gained specific LCC routing', state.get('updated', 0), state.get('unchanged', 0), state.get('failed', 0), state['refined'])
    logging.info('Subject placement: %s newly placed, %s remain unplaced', state['newly_placed'], state['remaining_unplaced'])
    if state['state'] == 'completed_with_failures':
        raise SystemExit(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--metadata-url', default='https://meta.bokheim.se')
    parser.add_argument('--workers', type=int, default=2)
    parser.add_argument('--batch-size', type=int, default=4)
    parser.add_argument('--publish-every', type=int, default=100)
    parser.add_argument('--limit', type=int)
    parser.add_argument('--missing-subjects', action='store_true', help='Enrich only entries with no projected subject placement, including entries without LCC codes')
    args = parser.parse_args()
    if min(args.workers, args.batch_size, args.publish_every, args.limit if args.limit is not None else 1) < 1 or args.batch_size > 8:
        parser.error('Worker, batch, publication and optional limit counts must be positive')
    run(args)


if __name__ == '__main__':
    main()
