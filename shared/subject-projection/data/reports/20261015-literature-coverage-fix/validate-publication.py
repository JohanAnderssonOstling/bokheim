#!/usr/bin/env python3
"""Compare the prepared publication with the installed catalog before installation."""
from collections import Counter, defaultdict
import csv
import hashlib
import json
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parent


def readonly(path):
    return sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)


def concepts(db):
    result = defaultdict(set)
    for key, concept in db.execute('SELECT lower(hex(b.key)),bc.concept_id FROM book b LEFT JOIN book_concept bc ON bc.book_row_id=b.row_id'):
        if concept is not None:
            result[key].add(concept)
        else:
            result[key]
    return result


def main():
    plan = json.loads((ROOT / 'publication-plan.json').read_text())
    artifact = Path(plan['output']) / 'releases' / str(plan['release_id']) / 'catalog.sqlite3'
    old, new = readonly(Path(plan['installed_catalog'])), readonly(artifact)
    assert new.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
    assert not new.execute('PRAGMA foreign_key_check').fetchall()
    revision = new.execute('SELECT taxonomy_revision FROM subject_projection').fetchone()[0]
    assert revision == '1:ad646d13-11df-52db-9fe9-50b6c091866e:147'
    differences = {}
    for table in ['source', 'description', 'book', 'author', 'book_author', 'book_language', 'book_identifier',
                  'subject_source', 'subject_authority', 'subject', 'book_subject', 'book_source', 'source_entry', 'source_entry_source']:
        before = Counter(old.execute('SELECT * FROM ' + table))
        after = Counter(new.execute('SELECT * FROM ' + table))
        if before != after:
            differences[table] = dict(removed=sum((before - after).values()), added=sum((after - before).values()))
    assert not differences, differences
    before, after = concepts(old), concepts(new)
    assert before.keys() == after.keys()
    unassigned = [key for key in before if before[key] and not after[key]]
    assert not unassigned, unassigned
    language_before = {key for key, ids in before.items() if ids == {3702}}
    language_after = {key for key, ids in after.items() if ids == {3702}}
    taxonomy = readonly(ROOT.parents[1] / 'unified-taxonomy-v2.sqlite3')
    labels = dict(taxonomy.execute('SELECT concept_id,preferred_label FROM concept'))
    examples = []
    changes = []
    for key, title, book_format in new.execute('SELECT lower(hex(key)),title,format FROM book'):
        if before[key] != after[key]:
            changes.append(dict(key=key, title=title, format=book_format,
                                before_ids='|'.join(map(str, sorted(before[key]))), after_ids='|'.join(map(str, sorted(after[key])))))
        if key in language_before:
            examples.append(dict(key=key, title=title, format=book_format,
                                 after_concepts=' | '.join(labels[identity] for identity in sorted(after[key]))))
    for name, rows, columns in [
        ('former-language-only-entries.csv', examples, ['key', 'title', 'format', 'after_concepts']),
        ('book-assignment-changes.csv', changes, ['key', 'title', 'format', 'before_ids', 'after_ids']),
    ]:
        with (ROOT / name).open('w', newline='') as file:
            writer = csv.DictWriter(file, fieldnames=columns)
            writer.writeheader()
            writer.writerows(rows)
    result = dict(release_id=plan['release_id'], previous_release_id=plan['previous_release_id'],
                  snapshot_sha256=hashlib.sha256(artifact.read_bytes()).hexdigest(), snapshot_bytes=artifact.stat().st_size,
                  books=len(after), changed_book_assignments=len(changes), language_only_before=len(language_before),
                  language_only_after=len(language_after), formerly_language_only_now_with_other_assignments=len(language_before - language_after),
                  became_unassigned=0, non_classification_table_differences=differences,
                  taxonomy_revision=revision, integrity_check='ok', foreign_key_violations=0)
    (ROOT / 'publication-validation.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
