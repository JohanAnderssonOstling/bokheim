#!/usr/bin/env python3
"""Apply a reviewed leaf-to-parent plan, rejecting changed labels, parents or codes."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sqlite3

TABLES = ('concept', 'concept_parent', 'lcc_selector', 'bisac_selector',
          'ddc_selector', 'lcc_range', 'taxonomy_meta', 'sqlite_sequence')
CODE_TABLES = ('lcc_selector', 'bisac_selector', 'ddc_selector', 'lcc_range')


def snapshot(db):
    return {table: Counter(db.execute(f'SELECT * FROM {table}')) for table in TABLES}


def absorb(db, plan):
    before = snapshot(db)
    labels = {row[0]: row[1] for row in before['concept']}
    merges = {}
    for group in plan['groups']:
        for source in group['sources']:
            old, target = source['id'], source['target']
            assert old not in merges and old != target
            assert labels[old] == source['label'] and labels[target] == source['target_label']
            assert db.execute('SELECT parent_concept_id FROM concept_parent WHERE concept_id=?', (old,)).fetchall() == [(target,)]
            assert not db.execute('SELECT 1 FROM concept_parent WHERE parent_concept_id=?', (old,)).fetchone()
            assert set(source['codes']) == set(CODE_TABLES)
            for table, rows in source['codes'].items():
                assert Counter(db.execute(f'SELECT * FROM {table} WHERE concept_id=?', (old,))) == Counter(map(tuple, rows)), (old, table)
            merges[old] = target
    assert len(merges) == plan['proposed_reduction']
    assert not (set(merges) & set(merges.values()))
    for old, target in merges.items():
        for table in ('lcc_selector', 'bisac_selector'):
            db.execute(f'INSERT OR IGNORE INTO {table} SELECT ?,selector FROM {table} WHERE concept_id=?', (target, old))
            db.execute(f'DELETE FROM {table} WHERE concept_id=?', (old,))
        for table in ('lcc_range', 'ddc_selector'):
            db.execute(f'UPDATE {table} SET concept_id=? WHERE concept_id=?', (target, old))
        db.execute('DELETE FROM concept_parent WHERE concept_id=?', (old,))
        db.execute('DELETE FROM concept WHERE concept_id=?', (old,))
    expected = dict(before)
    for table in ('concept', 'concept_parent'):
        expected[table] = Counter({row: count for row, count in before[table].items() if row[0] not in merges})
    for table in ('lcc_selector', 'bisac_selector'):
        expected[table] = Counter(set((merges.get(row[0], row[0]), *row[1:]) for row in before[table]))
    expected['lcc_range'] = Counter()
    for row, count in before['lcc_range'].items():
        expected['lcc_range'][(merges.get(row[0], row[0]), *row[1:])] += count
    expected['ddc_selector'] = Counter((code, merges.get(node, node)) for code, node in before['ddc_selector'])
    assert snapshot(db) == expected, 'unexpected changes outside the reviewed absorption plan'
    assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
    assert not db.execute('PRAGMA foreign_key_check').fetchall()
    return {
        'scope': 'Apply all user-approved groups from the saved plan, transferring all classification mappings to existing parents.',
        'before_concepts': sum(before['concept'].values()),
        'after_concepts': sum(expected['concept'].values()),
        'groups': [{'name': group['name'], 'absorbed_subjects': len(group['sources'])} for group in plan['groups']],
        'merges': {old: {'into': target, 'old_label': labels[old], 'surviving_label': labels[target]} for old, target in merges.items()},
        'validation': {'integrity_check': 'ok', 'foreign_key_violations': 0,
                       'exact_row_remapping': True, 'range_multiplicity_preserved': True,
                       'surviving_ids_and_autoincrement_preserved': True,
                       'only_childless_subjects_removed': True},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database', type=Path)
    parser.add_argument('plan', type=Path)
    parser.add_argument('manifest', type=Path)
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    with sqlite3.connect(args.database.resolve().as_uri() + '?mode=rw', uri=True) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('BEGIN IMMEDIATE')
        manifest = absorb(db, plan)
    manifest['plan'] = str(args.plan)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
    print(f"Absorbed {len(manifest['merges'])} leaves: {manifest['before_concepts']} -> {manifest['after_concepts']} subjects")


if __name__ == '__main__':
    main()
