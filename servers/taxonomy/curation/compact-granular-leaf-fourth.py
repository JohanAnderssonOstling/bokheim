#!/usr/bin/env python3
"""Apply the approved leaf batch to a copy, retaining three matcher conflicts by default."""
import argparse
import collections
import json
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[3]
PLAN = ROOT / 'shared/subject-projection/data/reports/20260920-granular-leaf-fourth-candidates.json'
TABLES = ('concept', 'concept_parent', 'lcc_selector', 'bisac_selector',
          'ddc_selector', 'lcc_range', 'taxonomy_meta', 'sqlite_sequence')
CONFLICTS = {
    7355: {'code': 'LB1044.87', 'actual_destination': 6806, 'actual_label': 'Audiovisual Teaching'},
    9846: {'code': 'LB1602', 'actual_destination': 53251, 'actual_label': 'Primary Vocational Education & Study Methods'},
    9827: {'code': 'T339', 'actual_destination': 7093, 'actual_label': 'History of patents'},
}


def snapshot(db):
    return {table: set(db.execute(f'SELECT * FROM {table}')) for table in TABLES}


def validate(db):
    assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
    assert not db.execute('PRAGMA foreign_key_check').fetchall()
    parents = collections.defaultdict(set)
    for child, parent in db.execute('SELECT concept_id,parent_concept_id FROM concept_parent'):
        parents[child].add(parent)
    done, visiting = set(), set()

    def visit(node):
        if node in done:
            return
        assert node not in visiting, f'cycle at {node}'
        visiting.add(node)
        for parent in parents[node]:
            visit(parent)
        visiting.remove(node)
        done.add(node)

    for (node,) in db.execute('SELECT concept_id FROM concept'):
        visit(node)


def compact(db, include_conflicts=False):
    before = snapshot(db)
    labels = {row[0]: row[1] for row in before['concept']}
    merges = {}
    for group in json.loads(PLAN.read_text())['groups']:
        target = group['target']
        assert labels[target] == group['target_label']
        for source in group['sources']:
            old = source['id']
            assert old not in merges and old != target
            assert labels[old] == source['label']
            assert db.execute('SELECT parent_concept_id FROM concept_parent WHERE concept_id=?', (old,)).fetchall() == [(target,)]
            assert not db.execute('SELECT 1 FROM concept_parent WHERE parent_concept_id=?', (old,)).fetchone()
            for table, rows in source['codes'].items():
                assert set(db.execute(f'SELECT * FROM {table} WHERE concept_id=?', (old,))) == set(map(tuple, rows)), (old, table)
            merges[old] = target
    assert len(merges) == 34
    deferred = {old: {'proposed_parent': merges[old], 'label': labels[old], **conflict}
                for old, conflict in CONFLICTS.items()} if not include_conflicts else {}
    merges = {old: target for old, target in merges.items() if old not in deferred}
    for old, target in merges.items():
        for table in ('lcc_selector', 'bisac_selector'):
            db.execute(f'INSERT OR IGNORE INTO {table} SELECT ?,selector FROM {table} WHERE concept_id=?', (target, old))
            db.execute(f'DELETE FROM {table} WHERE concept_id=?', (old,))
        for table in ('lcc_range', 'ddc_selector'):
            db.execute(f'UPDATE {table} SET concept_id=? WHERE concept_id=?', (target, old))
        db.execute('DELETE FROM concept_parent WHERE concept_id=?', (old,))
        db.execute('DELETE FROM concept WHERE concept_id=?', (old,))
    expected = dict(before)
    expected['concept'] = {row for row in before['concept'] if row[0] not in merges}
    expected['concept_parent'] = {row for row in before['concept_parent'] if row[0] not in merges}
    for table in ('lcc_selector', 'bisac_selector', 'lcc_range'):
        expected[table] = {(merges.get(row[0], row[0]), *row[1:]) for row in before[table]}
    expected['ddc_selector'] = {(code, merges.get(node, node)) for code, node in before['ddc_selector']}
    assert snapshot(db) == expected, 'unexpected changes outside approved merges'
    validate(db)
    return {
        'scope': f'Absorb {len(merges)} approved leaves into their existing parents, retaining all classification mappings and surviving IDs.',
        'before_concepts': len(before['concept']),
        'after_concepts': len(expected['concept']),
        'merges': {old: {'into': target, 'old_label': labels[old], 'surviving_label': labels[target]} for old, target in merges.items()},
        'deferred_matcher_conflicts': deferred,
        'validation': {'integrity_check': 'ok', 'foreign_key_violations': 0, 'acyclic': True, 'exact_row_remapping': True},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database', type=Path)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('--include-conflicts', action='store_true', help='Also merge three leaves whose codes resolve to overlapping retained subjects')
    args = parser.parse_args()
    with sqlite3.connect(args.database.resolve().as_uri() + '?mode=rw', uri=True) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('BEGIN IMMEDIATE')
        manifest = compact(db, include_conflicts=args.include_conflicts)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n')
    print(f"Merged {len(manifest['merges'])} leaves: {manifest['before_concepts']} -> {manifest['after_concepts']} concepts")


if __name__ == '__main__':
    main()
