#!/usr/bin/env python3
"""Flatten English Forms into Great Britain without changing subject identities."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[3]
PLAN = ROOT / 'shared/subject-projection/data/literature-english-forms-great-britain-20261015.json'


def snapshot(db):
    tables = [r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_stat%'")]
    return {table: Counter(db.execute(f'SELECT * FROM "{table}"')) for table in tables}


def migrate(db, plan):
    removed, parent = plan['removed_concept_id'], plan['parent_concept_id']
    children = {edge[0] for edge in plan['child_edges']}
    old_parent = plan['previous_parent_concept_id']
    child_edges = {tuple(edge) for edge in plan['child_edges']}
    before = snapshot(db)
    if not db.execute('SELECT 1 FROM concept WHERE concept_id=?', (removed,)).fetchone():
        assert {r[0] for r in db.execute('SELECT concept_id FROM concept_parent WHERE parent_concept_id=?', (parent,))} >= children
        return dict(state='already_applied')
    assert db.execute('SELECT preferred_label FROM concept WHERE concept_id=?', (removed,)).fetchone() == (plan['removed_label'],)
    assert db.execute('SELECT * FROM concept_parent WHERE concept_id=?', (removed,)).fetchall() == [(removed, old_parent, 0)]
    assert set(db.execute('SELECT * FROM concept_parent WHERE parent_concept_id=?', (removed,))) == child_edges
    for table in ['lcc_selector', 'bisac_selector', 'ddc_selector', 'lcc_range']:
        assert not db.execute(f'SELECT 1 FROM {table} WHERE concept_id=?', (removed,)).fetchone(), f'{table}: coded grouping cannot be removed'
    expected = {table: rows.copy() for table, rows in before.items()}
    concept_row = db.execute('SELECT * FROM concept WHERE concept_id=?', (removed,)).fetchone()
    expected['concept'][concept_row] -= 1
    expected['concept_parent'][removed, old_parent, 0] -= 1
    for child, _, ordinal in child_edges:
        expected['concept_parent'][child, removed, ordinal] -= 1
        expected['concept_parent'][child, parent, ordinal] += 1
    db.execute('UPDATE concept_parent SET parent_concept_id=? WHERE parent_concept_id=?', (parent, removed))
    db.execute('DELETE FROM concept WHERE concept_id=?', (removed,))
    assert snapshot(db) == {table: +rows for table, rows in expected.items()}, 'Unexpected changes outside the reviewed graph edges'
    assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
    assert not db.execute('PRAGMA foreign_key_check').fetchall()
    assert not db.execute('''SELECT c.normalized_label FROM concept_parent cp JOIN concept c USING(concept_id)
        WHERE cp.parent_concept_id=? GROUP BY c.normalized_label HAVING count(*)>1''', (parent,)).fetchall()
    return dict(state='applied', subjects_promoted=len(children), subject_ids_and_selectors_preserved=True, removed_uncoded_groups=1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database', type=Path)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--backup', type=Path)
    args = parser.parse_args()
    if args.apply:
        if not args.backup or args.backup.exists():
            parser.error('--apply requires a new --backup destination')
        db = sqlite3.connect(args.database)
        with sqlite3.connect(args.backup) as backup:
            db.backup(backup)
    else:
        db = sqlite3.connect(':memory:')
        with sqlite3.connect(args.database.resolve().as_uri()+'?mode=ro', uri=True) as source:
            source.backup(db)
    db.execute('PRAGMA foreign_keys=ON')
    db.execute('BEGIN IMMEDIATE')
    result = migrate(db, json.loads(PLAN.read_text()))
    db.commit()
    print(json.dumps(dict(preview=not args.apply, **result)))


if __name__ == '__main__':
    main()
