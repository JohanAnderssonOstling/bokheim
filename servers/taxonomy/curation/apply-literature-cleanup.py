#!/usr/bin/env python3
"""Preview or apply the reviewed literature-only taxonomy migration.

Existing subjects keep their identities. The manifest pins every edited label,
parent and classification boundary. Preview operates on an in-memory copy;
application requires a backup destination and rejects changed source rows.
"""

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import sqlite3

TABLES = ('concept', 'concept_parent', 'lcc_selector', 'lcc_range',
          'bisac_selector', 'ddc_selector', 'taxonomy_meta', 'sqlite_sequence')
DEFAULT_MANIFEST = (Path(__file__).resolve().parents[3] / 'shared' /
                    'subject-projection/data/literature-cleanup-20261014.json')


def snapshot(db):
    return {table: Counter(db.execute(f'SELECT * FROM {table}')) for table in TABLES}


def duplicate_siblings(rows):
    labels = {row[0]: row[1] for row in rows['concept']}
    siblings = defaultdict(list)
    for child, parent, _ in rows['concept_parent']:
        siblings[parent, labels[child]].append(child)
    return {key: tuple(sorted(ids)) for key, ids in siblings.items() if len(ids) > 1}


def migrate(db, plan):
    before = snapshot(db)
    expected = {table: rows.copy() for table, rows in before.items()}
    labels = {row[0]: row for row in before['concept']}
    parents = defaultdict(list)
    for child, parent, ordinal in sorted(before['concept_parent'], key=lambda row: (row[0], row[2])):
        parents[child].append(parent)
    sequence = {name: seq for name, seq in before['sqlite_sequence']}.get('concept')
    if sequence != plan['baseline_sequence']:
        raise ValueError(f'Concept sequence changed: expected {plan["baseline_sequence"]}, got {sequence}')

    # Freeze the complete Youth and Comics subtrees, including alternative
    # parent edges, against accidental reparenting or selector changes.
    protected = set(plan['protected_roots'])
    while True:
        expanded = protected | {child for child, parent, _ in before['concept_parent'] if parent in protected}
        if expanded == protected:
            break
        protected = expanded
    for edit in plan['edits']:
        identity = edit['id']
        if identity in protected:
            raise ValueError(f'Cannot edit protected Youth/Comics subject {identity}')
        if labels.get(identity, (None, None))[1] != edit['before_label'] or parents[identity] != edit['before_parents']:
            raise ValueError(f'Subject {identity} changed since review')
        old = labels[identity]
        new = (identity, edit['after_label'], ' '.join(edit['after_label'].split()).lower())
        expected['concept'][old] -= 1
        expected['concept'][new] += 1
        db.execute('UPDATE concept SET preferred_label=?,normalized_label=? WHERE concept_id=?', (new[1], new[2], identity))
        old_edges = [row for row in expected['concept_parent'] if row[0] == identity]
        for edge in old_edges:
            del expected['concept_parent'][edge]
        db.execute('DELETE FROM concept_parent WHERE concept_id=?', (identity,))
        # New parents are inserted after allocating new concepts below.
        for ordinal, parent in enumerate(edit['after_parents']):
            expected['concept_parent'][identity, parent, ordinal] += 1

    for concept in plan['new_concepts']:
        identity, label = concept['id'], concept['label']
        allocated = db.execute('INSERT INTO concept(preferred_label,normalized_label) VALUES (?,?) RETURNING concept_id',
                               (label, ' '.join(label.split()).lower())).fetchone()[0]
        if allocated != identity:
            raise ValueError(f'Expected new identity {identity}, allocated {allocated}')
        expected['concept'][identity, label, ' '.join(label.split()).lower()] += 1
        for ordinal, parent in enumerate(concept['parents']):
            expected['concept_parent'][identity, parent, ordinal] += 1

    for node in plan['new_concepts']:
        db.executemany('INSERT INTO concept_parent VALUES (?,?,?)',
                       [(node['id'], parent, ordinal) for ordinal, parent in enumerate(node['parents'])])
    for edit in plan['edits']:
        db.executemany('INSERT INTO concept_parent VALUES (?,?,?)',
                       [(edit['id'], parent, ordinal) for ordinal, parent in enumerate(edit['after_parents'])])
    for table, key in [('lcc_range', 'lcc_ranges'), ('lcc_selector', 'lcc_selectors'),
                       ('bisac_selector', 'bisac_selectors')]:
        for raw in plan.get('remove_' + key, []):
            row = tuple(raw)
            if row[0] in protected or before[table][row] != 1:
                raise ValueError(f'Changed or protected {table} row: {row}')
            columns = [r[1] for r in db.execute(f'PRAGMA table_info({table})')]
            count = db.execute(f'DELETE FROM {table} WHERE ' + ' AND '.join(f'{c} IS ?' for c in columns), row).rowcount
            if count != 1:
                raise ValueError(f'Expected exactly one {table} row, removed {count}')
            expected[table][row] -= 1
        for raw in plan.get('add_' + key, []):
            row = tuple(raw)
            if row[0] in protected:
                raise ValueError(f'Cannot change protected {table} row: {row}')
            db.execute(f'INSERT INTO {table} VALUES ({",".join("?" for _ in row)})', row)
            expected[table][row] += 1

    if plan['new_concepts']:
        expected['sqlite_sequence']['concept', sequence] -= 1
        expected['sqlite_sequence']['concept', plan['new_concepts'][-1]['id']] += 1
    expected = {table: +rows for table, rows in expected.items()}
    after = snapshot(db)
    if after != expected:
        raise ValueError('Unexpected changes outside the reviewed migration')
    if any(before['concept'][labels[i]] and i not in {r[0] for r in after['concept']} for i in labels):
        raise ValueError('Existing concept identities were removed')
    added_duplicates = set(duplicate_siblings(after).items()) - set(duplicate_siblings(before).items())
    if added_duplicates:
        raise ValueError(f'New duplicate sibling labels: {added_duplicates}')
    if db.execute('PRAGMA integrity_check').fetchone()[0] != 'ok' or db.execute('PRAGMA foreign_key_check').fetchall():
        raise ValueError('Taxonomy integrity or foreign-key check failed')

    after_parents = defaultdict(list)
    for child, parent, _ in after['concept_parent']:
        after_parents[child].append(parent)
    visiting = set()
    roots_cache = {}

    def roots(identity):
        if identity in visiting:
            raise ValueError(f'Taxonomy cycle at {identity}')
        if identity not in roots_cache:
            visiting.add(identity)
            roots_cache[identity] = set().union(*(roots(p) for p in after_parents[identity])) if after_parents[identity] else {identity}
            visiting.remove(identity)
        return roots_cache[identity]

    for identity, *_ in after['concept']:
        roots(identity)
    for identity in plan['moved_literary_concepts'] + [c['id'] for c in plan['new_concepts']]:
        if roots(identity) != {3846}:
            raise ValueError(f'Literary subject {identity} has a route outside Literature')
    return {'existing_subjects_moved': len(plan['moved_literary_concepts']),
            'new_subjects': len(plan['new_concepts']), 'reviewed_literary_scopes': len(plan['reviewed_scopes']),
            'protected_youth_comics_subjects': len(protected), 'exact_row_changes': True,
            'existing_ids_preserved': True, 'integrity_check': 'ok', 'foreign_key_violations': 0,
            'cycles': 0, 'new_duplicate_siblings': 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database', type=Path)
    parser.add_argument('--manifest', type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--backup', type=Path)
    args = parser.parse_args()
    if args.apply and not args.backup:
        parser.error('--apply requires --backup')
    plan = json.loads(args.manifest.read_text())
    if args.apply:
        if args.backup.exists():
            parser.error('backup already exists; choose a new destination')
        db = sqlite3.connect(args.database.resolve().as_uri() + '?mode=rw', uri=True)
        with sqlite3.connect(args.backup) as backup:
            db.backup(backup)
    else:
        db = sqlite3.connect(':memory:')
        with sqlite3.connect(args.database.resolve().as_uri() + '?mode=ro', uri=True) as source:
            source.backup(db)
    try:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('BEGIN IMMEDIATE')
        result = migrate(db, plan)
        db.commit()
        print(json.dumps({'applied': args.apply, **result}, indent=2))
    finally:
        db.close()


if __name__ == '__main__':
    main()
