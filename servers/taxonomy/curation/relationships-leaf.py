#!/usr/bin/env python3
"""Apply the reviewed Relationships leaf merge and independent Erotica move."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[3]
PLAN = ROOT / 'shared/subject-projection/data/literature-relationships-leaf-20261015.json'


def snapshot(db):
    return {name: Counter(db.execute(f'SELECT * FROM "{name}"'))
            for name, in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_stat%'")}


def migrate(db, plan):
    target, parent, promoted = (plan[key] for key in ['target_concept_id', 'parent_concept_id', 'promoted_concept_id'])
    merged = {item['concept_id'] for item in plan['merged_concepts']}
    before = snapshot(db)
    present = {row[0] for row in before['concept']}
    assert db.execute('SELECT preferred_label FROM concept WHERE concept_id=?', (target,)).fetchone() == (plan['target_label'],)
    if not present.intersection(merged):
        assert not db.execute('SELECT 1 FROM concept_parent WHERE parent_concept_id=?', (target,)).fetchone()
        assert db.execute('SELECT parent_concept_id FROM concept_parent WHERE concept_id=?', (promoted,)).fetchall() == [(parent,)]
        return dict(state='already_applied')
    assert merged <= present
    assert db.execute('SELECT * FROM concept_parent WHERE concept_id=?', (target,)).fetchall() == [(target, parent, 0)]
    assert db.execute('SELECT * FROM concept_parent WHERE concept_id=?', (promoted,)).fetchall() == [(promoted, target, 0)]
    assert {r[0] for r in db.execute('SELECT concept_id FROM concept_parent WHERE parent_concept_id=?', (target,))} == {promoted, 3983, 4002, 4058}
    assert all(child in merged or old_parent not in merged for child, old_parent, _ in before['concept_parent'])
    assert all(old_parent in merged or old_parent == target for child, old_parent, _ in before['concept_parent'] if child in merged)
    remap = lambda cid: target if cid in merged else cid
    expected = {table: rows.copy() for table, rows in before.items()}
    expected['concept'] = Counter(row for row in before['concept'] if row[0] not in merged)
    expected['concept_parent'] = Counter((child, parent if child == promoted else old_parent, ordinal)
                                         for child, old_parent, ordinal in before['concept_parent'] if child not in merged)
    for table in ['lcc_selector', 'bisac_selector']:
        expected[table] = Counter({(remap(cid), code) for cid, code in before[table]})
    expected['ddc_selector'] = Counter({(code, remap(cid)) for code, cid in before['ddc_selector']})
    expected['lcc_range'] = Counter({(remap(row[0]), *row[1:]) for row in before['lcc_range']})
    for table in ['concept_parent', 'lcc_selector', 'bisac_selector', 'ddc_selector', 'lcc_range', 'concept']:
        columns = [row[1] for row in db.execute(f'PRAGMA table_info({table})')]
        predicate = ' AND '.join(f'"{column}" IS ?' for column in columns)
        db.executemany(f'DELETE FROM {table} WHERE {predicate}', (before[table] - expected[table]).elements())
    for table in ['concept', 'concept_parent', 'lcc_selector', 'bisac_selector', 'ddc_selector', 'lcc_range']:
        rows = list((expected[table] - before[table]).elements())
        if rows:
            db.executemany(f'INSERT INTO {table} VALUES ({",".join("?" for _ in rows[0])})', rows)
    assert snapshot(db) == expected, 'Unexpected changes outside the reviewed merge'
    assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
    assert not db.execute('PRAGMA foreign_key_check').fetchall()
    assert not db.execute('SELECT 1 FROM concept_parent WHERE parent_concept_id=?', (target,)).fetchone()
    return dict(state='applied', merged_concepts=len(merged), erotica_descendants_preserved=len(plan['retained_erotica_concept_ids'])-1)


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
    # Apply the reviewed row changes transactionally; validate constraints before commit.
    db.execute('PRAGMA defer_foreign_keys=ON')
    result = migrate(db, json.loads(PLAN.read_text()))
    db.commit()
    print(json.dumps(dict(preview=not args.apply, **result)))


if __name__ == '__main__':
    main()
