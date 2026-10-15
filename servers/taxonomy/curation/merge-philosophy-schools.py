#!/usr/bin/env python3
"""Merge the reviewed Philosophy/Schools sibling into Traditions & Schools.

Retain subject 4507 and every descendant ID. Record the removed subject's
replacement in the manifest; leave the complete master reference unchanged.
"""

import argparse
from collections import Counter
import json
from pathlib import Path
import sqlite3


SOURCE = 4520
TARGET = 4507
TABLES = ("concept", "concept_parent", "lcc_selector", "bisac_selector",
          "ddc_selector", "lcc_range", "taxonomy_meta", "sqlite_sequence")


def snapshot(db):
    return {table: Counter(db.execute(f"SELECT * FROM {table}")) for table in TABLES}


def merge(db):
    before = snapshot(db)
    labels = {row[0]: row[1] for row in before["concept"]}
    assert labels[SOURCE] == "Schools"
    assert labels[TARGET] == "Philosophical Traditions & Schools"
    source_parents = db.execute(
        "SELECT parent_concept_id FROM concept_parent WHERE concept_id=?", (SOURCE,)
    ).fetchall()
    target_parents = db.execute(
        "SELECT parent_concept_id FROM concept_parent WHERE concept_id=?", (TARGET,)
    ).fetchall()
    assert source_parents == target_parents == [(54959,)]
    children = db.execute(
        "SELECT concept_id FROM concept_parent WHERE parent_concept_id=? ORDER BY concept_id",
        (SOURCE,),
    ).fetchall()
    assert children == [(4521,), (4522,), (4523,), (4529,), (4533,), (5533,)]
    assert not db.execute(
        "SELECT 1 FROM concept_parent WHERE parent_concept_id=? "
        "AND concept_id IN (SELECT concept_id FROM concept_parent WHERE parent_concept_id=?)",
        (SOURCE, TARGET),
    ).fetchone()

    # These siblings have no shared child edges. Changing their parent keeps
    # every child's other parents and parent ordinal intact.
    db.execute("UPDATE concept_parent SET parent_concept_id=? WHERE parent_concept_id=?",
               (TARGET, SOURCE))
    db.execute("DELETE FROM concept_parent WHERE concept_id=?", (SOURCE,))
    for table in ("lcc_selector", "bisac_selector"):
        db.execute(f"INSERT OR IGNORE INTO {table} SELECT ?,selector FROM {table} WHERE concept_id=?",
                   (TARGET, SOURCE))
        db.execute(f"DELETE FROM {table} WHERE concept_id=?", (SOURCE,))
    for table in ("ddc_selector", "lcc_range"):
        db.execute(f"UPDATE {table} SET concept_id=? WHERE concept_id=?", (TARGET, SOURCE))
    db.execute("DELETE FROM concept WHERE concept_id=?", (SOURCE,))

    # Compare all authoritative rows, not just the affected branch.
    expected = dict(before)
    expected["concept"] = Counter({r: n for r, n in before["concept"].items() if r[0] != SOURCE})
    expected["concept_parent"] = Counter(
        (child, TARGET if parent == SOURCE else parent, ordinal)
        for child, parent, ordinal in before["concept_parent"] if child != SOURCE)
    for table in ("lcc_selector", "bisac_selector"):
        expected[table] = Counter({(TARGET if node == SOURCE else node, selector)
                                   for node, selector in before[table]})
    expected["ddc_selector"] = Counter(
        (selector, TARGET if node == SOURCE else node) for selector, node in before["ddc_selector"])
    expected["lcc_range"] = Counter()
    for row, count in before["lcc_range"].items():
        expected["lcc_range"][(TARGET if row[0] == SOURCE else row[0], *row[1:])] += count
    assert snapshot(db) == expected, "Unexpected changes outside the requested merge"
    assert db.execute("PRAGMA integrity_check").fetchone() == ("ok",)
    assert not db.execute("PRAGMA foreign_key_check").fetchall()
    assert not db.execute("""
        WITH RECURSIVE ancestry(child,parent) AS (
            SELECT concept_id,parent_concept_id FROM concept_parent
            UNION
            SELECT a.child,p.parent_concept_id FROM ancestry a
            JOIN concept_parent p ON p.concept_id=a.parent
        ) SELECT 1 FROM ancestry WHERE child=parent LIMIT 1
    """).fetchone(), "Taxonomy cycle"
    return {
        "merges": {str(SOURCE): TARGET},
        "surviving_label": labels[TARGET],
        "moved_children": [{"id": child, "label": labels[child]} for child, in children],
        "retained_children": [{"id": child, "label": labels[child]} for child, in db.execute(
            "SELECT concept_id FROM concept_parent WHERE parent_concept_id=? ORDER BY concept_id",
            (TARGET,))],
        "bisac_selectors": [code for code, in db.execute(
            "SELECT selector FROM bisac_selector WHERE concept_id=? ORDER BY selector", (TARGET,))],
        "ddc_selectors": [code for code, in db.execute(
            "SELECT selector FROM ddc_selector WHERE concept_id=? ORDER BY selector", (TARGET,))],
        "validation": {"exact_row_remapping": True, "integrity_check": "ok",
                       "foreign_key_violations": 0, "cycles": 0,
                       "surviving_ids_and_autoincrement_preserved": True},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=rw", uri=True) as db:
        db.execute("PRAGMA foreign_keys=ON")
        db.execute("BEGIN IMMEDIATE")
        result = merge(db)
    args.manifest.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(f"Merged {SOURCE} into {TARGET}; retained {len(result['retained_children'])} child categories")


if __name__ == "__main__":
    main()
