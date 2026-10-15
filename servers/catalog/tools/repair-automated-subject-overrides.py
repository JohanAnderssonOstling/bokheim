#!/usr/bin/env python3
"""Undo audited automatic-to-manual promotions and stage subject enrichment.

By default, only controls introduced by a supplied before/after audit are
removed. An explicit --clear-selected-manual instead removes all manual
controls from the threshold selection, including exclusions. A prior selection
can retain the original threshold cohort across reruns. Every non-subject field
survives. This tool edits an isolated copy; installation is separate.
"""
import argparse
import collections
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3


def load_rerun():
    path = Path(__file__).with_name('rerun-title-author-subjects.py')
    spec = importlib.util.spec_from_file_location('catalog_subject_rerun', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def identity(subject):
    return tuple(subject.get(field) for field in ('source', 'authority', 'code', 'id'))


def fingerprint(raw):
    return hashlib.sha256(raw.encode()).hexdigest()


def prepare(args, rerun):
    output = args.output
    output.mkdir(parents=True, exist_ok=True)
    stage = output / 'catalog-input'
    stage.mkdir()
    with rerun.readonly(args.catalog / 'curation.sqlite3') as source:
        for target in [output / 'curation-before.sqlite3', stage / 'curation.sqlite3']:
            with sqlite3.connect(target) as destination:
                source.backup(destination)
                destination.execute('PRAGMA journal_mode=DELETE')
    with rerun.readonly(args.snapshot) as snapshot:
        with sqlite3.connect(output / 'installed-before.sqlite3') as destination:
            snapshot.backup(destination)
        high_counts = dict(snapshot.execute('''
            SELECT lower(hex(b.key)), count(DISTINCT c.concept_id)
            FROM book b JOIN book_concept c ON c.book_row_id=b.row_id
            GROUP BY b.row_id HAVING count(DISTINCT c.concept_id)>?
        ''', (args.over_subjects,)))
    current_high_counts = high_counts.copy()
    if args.selection_from:
        prior_selection = json.loads(args.selection_from.read_text())
        for entry in prior_selection['entries']:
            count = entry.get('previous_subject_count')
            if count is not None and count > args.over_subjects:
                high_counts.setdefault(entry['key'], count)
    audits = json.loads(args.audit.read_text()) if args.audit else []
    promotions = {}
    for row in audits:
        before = row['before_record']['book'].get('subjects', [])
        after = row['after_record']['book'].get('subjects', [])
        prior = {identity(subject) for subject in before}
        added = {identity(subject) for subject in after
                 if subject.get('source', '').startswith('manual:')
                 and identity(subject) not in prior}
        promotions[row['hash']] = added
    selected, requests, removed_controls, protected = [], [], [], []
    with sqlite3.connect(stage / 'curation.sqlite3') as db:
        db.execute('BEGIN IMMEDIATE')
        for table, column in [('book', 'hash'), ('external_book', 'entry_key')]:
            for key, raw in db.execute(f'SELECT {column}, record FROM {table} ORDER BY {column}').fetchall():
                original = json.loads(raw)
                record = copy.deepcopy(original)
                subjects = record['book'].get('subjects', [])
                introduced = promotions.get(key, set()) if table == 'book' else set()
                if args.clear_selected_manual and key in high_counts:
                    introduced = introduced | {identity(subject) for subject in subjects
                                                if subject.get('source', '').startswith('manual:')}
                removed = [subject for subject in subjects if identity(subject) in introduced]
                retained = [subject for subject in subjects if identity(subject) not in introduced]
                if key not in high_counts and not removed:
                    continue
                record['book']['subjects'] = retained
                if removed:
                    removed_controls.append({'table': table, 'key': key, 'title': record['title'], 'removed': removed})
                    db.execute(f'UPDATE {table} SET record=? WHERE {column}=?',
                               (json.dumps(record, ensure_ascii=False), key))
                manual = [subject for subject in retained if subject.get('source', '').startswith('manual:')]
                if manual:
                    protected.append({'table': table, 'key': key, 'subjects': manual})
                selected.append({'table': table, 'key': key, 'title': record['title'],
                                 'original_record_sha256': fingerprint(raw),
                                 'before_subjects': original['book'].get('subjects', []),
                                 'previous_subject_count': high_counts.get(key),
                                 'manual_controls_retained': manual})
                record['book']['subjects'] = [subject for subject in retained if not rerun.replaceable(subject)]
                record['description'] = ''
                audio = table == 'external_book' and db.execute(
                    'SELECT format FROM external_book WHERE entry_key=?', (key,)).fetchone()[0] == 16
                requests.append({'key': table + ':' + key, 'record': record,
                                 'audiobook': audio, 'needs_cover': False,
                                 'readings': [], 'recording': None})
        db.commit()
    assert set(high_counts) <= {row['key'] for row in selected}
    rerun.write_json(output / 'selection.json', {'entries': selected, 'count': len(selected),
                     'over_subjects': args.over_subjects, 'high_subject_entries': len(high_counts),
                     'current_high_subject_entries': len(current_high_counts),
                     'clear_selected_manual': args.clear_selected_manual,
                     'selection_from': str(args.selection_from.resolve()) if args.selection_from else None,
                     'audit': str(args.audit.resolve()) if args.audit else None,
                     'audit_sha256': hashlib.sha256(args.audit.read_bytes()).hexdigest() if args.audit else None})
    removed_name = 'removed-manual-controls.json' if args.clear_selected_manual else 'removed-automated-controls.json'
    rerun.write_json(output / removed_name, removed_controls)
    rerun.write_json(output / 'retained-manual-controls.json', protected)
    rerun.write_json(output / 'request.json', {'books': requests})
    rerun.refresh_facts(args.catalog, output)
    print(json.dumps({'selected': len(selected), 'over_threshold': len(high_counts),
                      'repaired_books': len(removed_controls),
                      'removed_controls': sum(len(row['removed']) for row in removed_controls),
                      'entries_with_independent_manual_controls': len(protected)}))


def apply(args, rerun):
    selection = json.loads((args.output / 'selection.json').read_text())
    response = json.loads(args.results.read_text())
    if response.get('policy') not in {'books_v25', 'books_v26', 'books_v27'}:
        raise ValueError('Expected a supported current subject policy')
    results = {row['key']: row for row in response['results']}
    expected = {row['table'] + ':' + row['key'] for row in selection['entries']}
    if len(results) != len(response['results']) or set(results) != expected:
        raise ValueError('Response omitted, duplicated, or added entries')
    if any(row['status'] == 'failed' for row in results.values()):
        raise ValueError('An enrichment request failed; retain the staged copy for retry')
    changes, counts = [], collections.Counter()
    with sqlite3.connect(args.output / 'catalog-input/curation.sqlite3') as db:
        db.execute('BEGIN IMMEDIATE')
        for row in selection['entries']:
            table, key = row['table'], row['key']
            column = 'hash' if table == 'book' else 'entry_key'
            record = json.loads(db.execute(f'SELECT record FROM {table} WHERE {column}=?', (key,)).fetchone()[0])
            subjects = [subject for subject in record['book'].get('subjects', []) if not rerun.replaceable(subject)]
            result = results[table + ':' + key]
            received = (result.get('record') or {}).get('book', {}).get('subjects', [])
            for subject in received:
                source = subject.get('source', '')
                managed = source.startswith(('metadata:', 'openlibrary:', 'library_of_congress:',
                                             'librarything:', 'inspection:verified-print-isbn:', 'inspection:isbn-reference:'))
                if managed and (subject.get('authority') or '').lower() in {'lcc', 'ddc', 'bisac', 'unified'} and subject not in subjects:
                    subjects.append(subject)
            manual = [subject for subject in subjects if subject.get('source', '').startswith('manual:')]
            assert manual == row['manual_controls_retained']
            record['book']['subjects'] = subjects
            changed = subjects != row['before_subjects']
            if changed:
                db.execute(f'UPDATE {table} SET record=? WHERE {column}=?', (json.dumps(record, ensure_ascii=False), key))
            counts['changed' if changed else 'unchanged'] += 1
            counts['status:' + result['status']] += 1
            changes.append({**row, 'changed': changed, 'status': result['status'],
                            'after_subjects': subjects,
                            'removed': [s for s in row['before_subjects'] if s not in subjects],
                            'added': [s for s in subjects if s not in row['before_subjects']]})
        db.commit()
    rerun.write_json(args.output / 'changes.json', changes)
    rerun.write_json(args.output / 'rerun-summary.json', {'policy': response['policy'], 'selected': len(changes), **counts})
    print(json.dumps(dict(counts)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    prep = commands.add_parser('prepare')
    prep.add_argument('--catalog', type=Path, required=True)
    prep.add_argument('--snapshot', type=Path, required=True)
    prep.add_argument('--audit', type=Path)
    prep.add_argument('--clear-selected-manual', action='store_true',
                      help='Explicitly remove all selected manual assignments and exclusions')
    prep.add_argument('--selection-from', type=Path,
                      help='Also include the prior selection entries originally over the threshold')
    prep.add_argument('--over-subjects', type=int, default=3)
    prep.add_argument('--output', type=Path, required=True)
    apply_parser = commands.add_parser('apply')
    apply_parser.add_argument('--output', type=Path, required=True)
    apply_parser.add_argument('--results', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'prepare' and not args.audit and not args.clear_selected_manual:
        parser.error('prepare requires --audit or an explicit --clear-selected-manual')
    {'prepare': prepare, 'apply': apply}[args.command](args, load_rerun())


if __name__ == '__main__':
    main()
