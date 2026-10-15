#!/usr/bin/env python3
"""Stage a subject-only rerun for catalog entries with title/author classification provenance."""
import argparse
import collections
import copy
import csv
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import xml.etree.ElementTree as ET
import zipfile

TITLE_SOURCES = frozenset({
    'metadata:work', 'metadata:shared-edition-classification',
    'metadata:year-only-edition-consensus', 'openlibrary:metadata:sibling-edition',
    'openlibrary:metadata:work',
    'metadata:edition-subject-consensus',
})
REPLACEABLE_SOURCES = TITLE_SOURCES | {'openlibrary:metadata:work'}
CONSENSUS_SOURCE = 'metadata:edition-subtree-consensus'


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


def readonly(path):
    return sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)


def is_lcc(subject):
    return (subject.get('authority') or '').lower() == 'lcc'


def replaceable(subject):
    classification = (subject.get('authority') or '').lower() in {'lcc', 'ddc', 'bisac', 'unified'}
    return (classification and subject.get('source') in REPLACEABLE_SOURCES) or subject.get('source') == CONSENSUS_SOURCE


def manual_subjects(subjects):
    return [s for s in subjects if s.get('source', '').startswith('manual:')]


def canonical_isbn(value):
    value = re.sub(r'[^0-9Xx]', '', value.removeprefix('urn:isbn:').removeprefix('ISBN:'))
    if len(value) == 10:
        digits = [10 if c.upper() == 'X' else int(c) for c in value]
        if sum((10 - i) * digit for i, digit in enumerate(digits)) % 11:
            return None
        value = '978' + value[:9]
        value += str(-sum(int(c) * (1 if i % 2 == 0 else 3) for i, c in enumerate(value)) % 10)
    return value if len(value) == 13 and value.isdigit() and sum(int(c) * (1 if i % 2 == 0 else 3) for i, c in enumerate(value)) % 10 == 0 else None


def refresh_facts(catalog, output):
    """An ISBN added by an old title match cannot establish our exact edition."""
    request_path = output / 'request.json'
    request = json.loads(request_path.read_text())
    if not (output / 'request-with-prior-identifiers.json').exists():
        write_json(output / 'request-with-prior-identifiers.json', request)
    changes, skipped = [], []
    with readonly(output / 'catalog-input/curation.sqlite3') as db:
        for facts in request['books']:
            table, key = facts['key'].split(':', 1)
            identifiers = facts['record']['book'].get('identifiers', [])
            if not any(i.get('scheme') == 'isbn' for i in identifiers):
                continue
            supported = None
            if table == 'book':
                path = catalog / 'books' / (key + '.epub')
                try:
                    with zipfile.ZipFile(path) as archive:
                        container = ET.fromstring(archive.read('META-INF/container.xml'))
                        opf = next(e.attrib['full-path'] for e in container.iter() if e.tag.endswith('}rootfile'))
                        metadata = ET.fromstring(archive.read(opf))
                        supported = {isbn for e in metadata.iter() if e.tag.endswith('}identifier') and (isbn := canonical_isbn(e.text or ''))}
                except (OSError, KeyError, ValueError, StopIteration, zipfile.BadZipFile, ET.ParseError) as error:
                    skipped.append({'key': facts['key'], 'error': str(error)})
            else:
                source = db.execute('SELECT source FROM external_book WHERE entry_key=?', (key,)).fetchone()[0]
                if source == 'loyalbooks.com':
                    # These are the source's Gutenberg/LibriVox entries. Their
                    # source metadata carries no ISBN edition identity.
                    metadata = [json.loads(row[0]) for row in db.execute('SELECT metadata FROM source_entry WHERE external_entry_key=?', (key,))]
                    supported = set()
                    for record in metadata:
                        for field in ('isbn', 'isbn10', 'isbn13', 'isbns'):
                            values = record.get(field, [])
                            values = values if isinstance(values, list) else [values]
                            supported.update(isbn for value in values if isinstance(value, str) and (isbn := canonical_isbn(value)))
            if supported is None:
                continue
            removed = [i for i in identifiers if i.get('scheme') == 'isbn' and canonical_isbn(i['value']) not in supported]
            if removed:
                facts['record']['book']['identifiers'] = [i for i in identifiers if i not in removed]
                changes.append({'key': facts['key'], 'title': facts['record']['title'], 'excluded_request_identifiers': removed, 'embedded_isbns': sorted(supported)})
    write_json(output / 'request-identifier-audit.json', {'changed_requests': len(changes), 'changes': changes, 'uninspected': skipped})
    write_json(request_path, request)
    write_json(output / 'example-request.json', {'books': [b for b in request['books'] if b['record']['title'] in {'Don Quijote', 'Dracula', 'Gulliver’s Travels'}]})
    print(json.dumps({'requests_with_unproven_isbns_excluded': len(changes), 'uninspected': len(skipped)}))


def prepare(catalog, output, without_manual=False, selection_from=None):
    output.mkdir(parents=True, exist_ok=True)
    staged = output / 'catalog-input'
    staged.mkdir(exist_ok=True)
    destination = staged / 'curation.sqlite3'
    if destination.exists():
        raise ValueError('Use a new output directory; keep the original staged input')
    with readonly(catalog / 'curation.sqlite3') as source, sqlite3.connect(destination) as target:
        source.backup(target)
    selected, books, protected = [], [], []
    prior_keys = None if selection_from is None else {(e['table'], e['key']) for e in json.loads(selection_from.read_text())['entries']}
    with readonly(destination) as db:
        for table, key_column in [('book', 'hash'), ('external_book', 'entry_key')]:
            for key, raw in db.execute(f'SELECT {key_column},record FROM {table} ORDER BY {key_column}'):
                record = json.loads(raw)
                manual = manual_subjects(record['book'].get('subjects', []))
                if manual:
                    protected.append({'table': table, 'key': key, 'title': record['title'], 'manual_assignments': [s for s in manual if s.get('source') != 'manual:subject:exclude'], 'manual_exclusions': [s for s in manual if s.get('source') == 'manual:subject:exclude'], 'record_sha256': hashlib.sha256(raw.encode()).hexdigest()})
                    if without_manual:
                        continue
                sources = {s['source'] for s in record['book'].get('subjects', []) if replaceable(s)}
                if prior_keys is not None:
                    if (table, key) not in prior_keys:
                        continue
                elif not sources & TITLE_SOURCES and not any(s.get('source') == CONSENSUS_SOURCE for s in record['book'].get('subjects', [])):
                    continue
                clean = copy.deepcopy(record)
                removed = [s for s in clean['book'].get('subjects', []) if replaceable(s)]
                clean['book']['subjects'] = [s for s in clean['book'].get('subjects', []) if not replaceable(s)]
                clean['description'] = ''
                identity = table + ':' + key
                selected.append({'table': table, 'key': key, 'title': record['title'], 'record_sha256': hashlib.sha256(raw.encode()).hexdigest(), 'removed_subjects': removed})
                books.append({'key': identity, 'record': clean, 'audiobook': False, 'needs_cover': False, 'readings': [], 'recording': None})
    write_json(output / 'selection.json', {'entries': selected, 'count': len(selected), 'source_catalog': str(catalog.resolve()), 'without_manual': without_manual})
    write_json(output / 'manual-assignments.json', protected)
    with (output / 'manual-assignments.csv').open('w', newline='') as stream:
        writer = csv.writer(stream)
        writer.writerow(['table', 'key', 'title', 'assignment_count', 'exclusion_count', 'assignments', 'exclusions'])
        for entry in protected:
            writer.writerow([entry['table'], entry['key'], entry['title'], len(entry['manual_assignments']), len(entry['manual_exclusions']), ' | '.join(s.get('code') or s['name'] for s in entry['manual_assignments']), ' | '.join(s.get('code') or s['name'] for s in entry['manual_exclusions'])])
    write_json(output / 'request.json', {'books': books})
    examples = [b for b in books if b['record']['title'] in {'Don Quijote', 'Dracula', 'Gulliver’s Travels'}]
    write_json(output / 'example-request.json', {'books': examples})
    refresh_facts(catalog, output)
    print(json.dumps({'selected': len(selected), 'tables': dict(collections.Counter(e['table'] for e in selected)), 'examples': len(examples)}))


def apply(output, results_path):
    selection = json.loads((output / 'selection.json').read_text())
    request = {b['key']: b for b in json.loads((output / 'request.json').read_text())['books']}
    response = json.loads(results_path.read_text())
    if response.get('policy') not in {'books_v23', 'books_v24', 'books_v25', 'books_v26', 'books_v27'}:
        raise ValueError('The rerun requires a supported corrected books policy')
    results = {r['key']: r for r in response['results']}
    if len(results) != len(response['results']) or set(results) != set(request):
        raise ValueError('The enrichment response omitted, duplicated, or added books')
    failed = [r for r in results.values() if r['status'] == 'failed']
    if failed:
        write_json(output / 'failures.json', failed)
        raise ValueError(f'{len(failed)} enrichment requests failed; no catalog changes applied')
    changes, counts = [], collections.Counter()
    with sqlite3.connect(output / 'catalog-input/curation.sqlite3') as db:
        db.execute('BEGIN IMMEDIATE')
        for entry in selection['entries']:
            table = entry['table']
            column = 'hash' if table == 'book' else 'entry_key'
            identity = table + ':' + entry['key']
            raw = db.execute(f'SELECT record FROM {table} WHERE {column}=?', (entry['key'],)).fetchone()[0]
            if hashlib.sha256(raw.encode()).hexdigest() != entry['record_sha256']:
                raise ValueError('Staged record changed since selection: ' + identity)
            record = json.loads(raw)
            old = record['book'].get('subjects', [])
            if selection.get('without_manual') and manual_subjects(old):
                raise ValueError('Manual subject controls appeared on a selected entry: ' + identity)
            result = results[identity]
            received = (result.get('record') or request[identity]['record'])['book'].get('subjects', [])
            subjects = [s for s in old if not replaceable(s)]
            new_classifications = []
            for subject in received:
                authority = (subject.get('authority') or '').lower()
                source = subject.get('source', '')
                managed = source.startswith(('metadata:', 'openlibrary:', 'library_of_congress:', 'librarything:', 'inspection:verified-print-isbn:', 'inspection:isbn-reference:'))
                if authority in {'lcc', 'ddc', 'bisac', 'unified'} and managed and subject not in subjects:
                    subjects.append(subject)
                    new_classifications.append(subject)
            # A failed lookup is not evidence that the old classifications are
            # wrong. Keep them for review when no replacement evidence exists.
            # Under an explicit consensus rerun, unsupported borrowed evidence
            # is withdrawn even when no replacement class reaches the threshold.
            # Original file/source subjects and strong classification sources stay.
            retained_previous = not selection.get('without_manual') and not new_classifications and not any(is_lcc(s) for s in subjects)
            if retained_previous:
                subjects = old
                counts['retained_without_new_classification'] += 1
            record['book']['subjects'] = subjects
            changed = subjects != old
            if changed:
                db.execute(f'UPDATE {table} SET record=? WHERE {column}=?', (json.dumps(record, ensure_ascii=False), entry['key']))
            counts['changed' if changed else 'unchanged'] += 1
            counts['status:' + result['status']] += 1
            new_lcc = [s for s in subjects if is_lcc(s)]
            counts['with_lcc' if new_lcc else 'without_lcc'] += 1
            counts['declared_work'] += any(s.get('source') == 'openlibrary:metadata:declared-work' for s in new_lcc)
            changes.append({'table': table, 'key': entry['key'], 'title': entry['title'], 'changed': changed, 'status': result['status'], 'retained_previous': retained_previous, 'removed': [s for s in old if s not in subjects], 'added': [s for s in subjects if s not in old], 'lcc': new_lcc})
        db.commit()
    write_json(output / 'changes.json', changes)
    write_json(output / 'rerun-summary.json', {'policy': response['policy'], 'selected': selection['count'], **counts})
    print(json.dumps({'selected': selection['count'], **counts}))


def retain_unmapped(catalog, output):
    """Retain prior subjects when publication shows no navigable replacement."""
    entries = json.loads((output / 'newly-unassigned.json').read_text())
    selected = {e['key']: e for e in json.loads((output / 'selection.json').read_text())['entries']}
    changes = json.loads((output / 'changes.json').read_text())
    by_key = {e['key']: e for e in changes}
    count = 0
    with readonly(catalog / 'curation.sqlite3') as original, sqlite3.connect(output / 'catalog-input/curation.sqlite3') as staged:
        staged.execute('BEGIN IMMEDIATE')
        for entry in entries:
            key = entry['key']
            selection = selected[key]
            change = by_key[key]
            if change.get('retention_reason') == 'replacement_has_no_taxonomy_assignment':
                continue
            assert change['changed'] and not change['retained_previous']
            table = selection['table']
            column = 'hash' if table == 'book' else 'entry_key'
            raw = original.execute(f'SELECT record FROM {table} WHERE {column}=?', (key,)).fetchone()[0]
            assert hashlib.sha256(raw.encode()).hexdigest() == selection['record_sha256'], 'Original record changed during the audit'
            record = json.loads(staged.execute(f'SELECT record FROM {table} WHERE {column}=?', (key,)).fetchone()[0])
            record['book']['subjects'] = json.loads(raw)['book']['subjects']
            staged.execute(f'UPDATE {table} SET record=? WHERE {column}=?', (json.dumps(record, ensure_ascii=False), key))
            change.update(changed=False, retained_previous=True, retention_reason='replacement_has_no_taxonomy_assignment', rejected_added_subjects=change['added'], added=[], removed=[], lcc=[s for s in record['book']['subjects'] if is_lcc(s)])
            count += 1
        staged.commit()
    summary = json.loads((output / 'rerun-summary.json').read_text())
    summary['changed'] -= count
    summary['unchanged'] += count
    summary['retained_without_new_classification'] += count
    summary['retained_unmapped_replacements'] = summary.get('retained_unmapped_replacements', 0) + count
    write_json(output / 'changes.json', changes)
    write_json(output / 'rerun-summary.json', summary)
    print(json.dumps({'retained_unmapped': count, 'retained_total': summary['retained_without_new_classification']}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    prep = sub.add_parser('prepare')
    prep.add_argument('--catalog', type=Path, required=True)
    prep.add_argument('--output', type=Path, required=True)
    prep.add_argument('--without-manual', action='store_true', help='Protect entries with any manual assignment or exclusion and audit their identities')
    prep.add_argument('--selection-from', type=Path, help='Reevaluate a prior selection, including entries whose borrowed subjects were withdrawn')
    ap = sub.add_parser('apply')
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--results', type=Path, required=True)
    facts = sub.add_parser('refresh-facts')
    facts.add_argument('--catalog', type=Path, required=True)
    facts.add_argument('--output', type=Path, required=True)
    retain = sub.add_parser('retain-unmapped')
    retain.add_argument('--catalog', type=Path, required=True)
    retain.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'prepare':
        prepare(args.catalog, args.output, args.without_manual, args.selection_from)
    elif args.command == 'apply':
        apply(args.output, args.results)
    elif args.command == 'retain-unmapped':
        retain_unmapped(args.catalog, args.output)
    else:
        refresh_facts(args.catalog, args.output)


if __name__ == '__main__':
    main()
