#!/usr/bin/env python3
"""Absorb the approved 149 author shelves into their reviewed current era parents."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[3]
REVIEW = ROOT / 'shared/subject-projection/data/reports/20261007-individual-authors-review.json'
TABLES = ('concept', 'concept_parent', 'lcc_selector', 'bisac_selector',
          'ddc_selector', 'lcc_range', 'taxonomy_meta', 'sqlite_sequence')


def snapshot(db):
    return {table: Counter(db.execute(f'SELECT * FROM {table}')) for table in TABLES}


def absorb(db):
    before = snapshot(db)
    labels = {row[0]: row[1] for row in before['concept']}
    authors = [author for author in json.loads(REVIEW.read_text())['authors']
               if author['batch'] in ('american-top50', 'english-top100')]
    assert Counter(author['batch'] for author in authors) == {'american-top50': 50, 'english-top100': 99}
    merges = {}
    for author in authors:
        old = author['id']
        assert labels[old] == author['label']
        assert len(author['current_parents']) == 1
        parent = author['current_parents'][0]
        target = parent['id']
        assert labels[target] == parent['label'] and target != old
        assert db.execute('SELECT parent_concept_id FROM concept_parent WHERE concept_id=?', (old,)).fetchall() == [(target,)]
        assert not db.execute('SELECT 1 FROM concept_parent WHERE parent_concept_id=?', (old,)).fetchone()
        for table, rows in author['selectors'].items():
            assert Counter(db.execute(f'SELECT * FROM {table} WHERE concept_id=?', (old,))) == Counter(map(tuple, rows)), (old, table)
        assert old not in merges
        merges[old] = target
    assert len(merges) == 149 and not (set(merges) & set(merges.values()))
    for old, target in merges.items():
        for table in ('lcc_selector', 'bisac_selector'):
            db.execute(f'INSERT OR IGNORE INTO {table} SELECT ?,selector FROM {table} WHERE concept_id=?', (target, old))
            db.execute(f'DELETE FROM {table} WHERE concept_id=?', (old,))
        for table in ('lcc_range', 'ddc_selector'):
            db.execute(f'UPDATE {table} SET concept_id=? WHERE concept_id=?', (target, old))
        db.execute('DELETE FROM concept_parent WHERE concept_id=?', (old,))
        db.execute('DELETE FROM concept WHERE concept_id=?', (old,))
    expected = dict(before)
    expected['concept'] = Counter({row: count for row, count in before['concept'].items() if row[0] not in merges})
    expected['concept_parent'] = Counter({row: count for row, count in before['concept_parent'].items() if row[0] not in merges})
    for table in ('lcc_selector', 'bisac_selector'):
        expected[table] = Counter(set((merges.get(row[0], row[0]), *row[1:]) for row in before[table]))
    expected['lcc_range'] = Counter()
    for row, count in before['lcc_range'].items():
        expected['lcc_range'][(merges.get(row[0], row[0]), *row[1:])] += count
    expected['ddc_selector'] = Counter((code, merges.get(node, node)) for code, node in before['ddc_selector'])
    assert snapshot(db) == expected, 'unexpected changes outside reviewed author absorptions'
    assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
    assert not db.execute('PRAGMA foreign_key_check').fetchall()
    for author in json.loads(REVIEW.read_text())['authors']:
        if author['batch'] == 'existing-topic':
            for table, rows in author['selectors'].items():
                assert Counter(db.execute(f'SELECT * FROM {table} WHERE concept_id=?', (author['id'],))) == Counter(map(tuple, rows))
    return {
        'scope': 'User-approved absorption of 50 American and 99 English author leaves into their current era parents. Goethe and the three Shakespeare subjects retained.',
        'before_concepts': sum(before['concept'].values()),
        'after_concepts': sum(expected['concept'].values()),
        'merges': {old: {'into': target, 'old_label': labels[old], 'surviving_label': labels[target]} for old, target in merges.items()},
        'validation': {'integrity_check': 'ok', 'foreign_key_violations': 0,
                       'exact_row_remapping': True, 'range_multiplicity_preserved': True,
                       'surviving_ids_and_autoincrement_preserved': True,
                       'only_childless_subjects_removed': True},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database', type=Path)
    parser.add_argument('manifest', type=Path)
    args = parser.parse_args()
    with sqlite3.connect(args.database.resolve().as_uri() + '?mode=rw', uri=True) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('BEGIN IMMEDIATE')
        manifest = absorb(db)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
    print(f"Absorbed {len(manifest['merges'])} authors: {manifest['before_concepts']} -> {manifest['after_concepts']} subjects")


if __name__ == '__main__':
    main()
