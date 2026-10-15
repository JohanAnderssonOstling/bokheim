import importlib.util
import json
from pathlib import Path
import sqlite3
import unittest


SPEC = importlib.util.spec_from_file_location('refine_subjects', Path(__file__).parents[1] / 'tools/refine-subjects.py')
refine = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(refine)


class SubjectBackfillTests(unittest.TestCase):
    def test_missing_subjects_includes_records_without_lcc_and_unresolved_codes(self):
        uncoded = dict(key='uncoded', needs_refinement=False, projected_paths=[])
        unresolved = dict(key='unresolved', needs_refinement=True, projected_paths=[])
        broad = dict(key='broad', needs_refinement=True, projected_paths=['Literature'])
        placed = dict(key='placed', needs_refinement=False, projected_paths=['Literature / Poetry'])
        audit = [uncoded, unresolved, broad, placed]
        self.assertEqual(refine.select_entries(audit), [uncoded, unresolved, broad])
        self.assertEqual(refine.select_entries(audit, missing_subjects=True), [uncoded, unresolved])

    def test_subject_only_merge_preserves_latest_metadata_and_is_idempotent(self):
        original = dict(name='Original heading', authority=None, code=None, source='dc:subject')
        manual = dict(name='Manual classification', authority='bisac', code='FIC000000', source='manual:subject')
        supported = dict(name='PR4034', authority='lcc', code='PR4034', source='metadata:verified')
        unsupported = dict(name='ZZ1234', authority='lcc', code='ZZ1234', source='metadata:verified')
        broad = dict(name='PR', authority='lcc', code='PR', source='metadata:verified')
        returned = dict(title='Wrong title', contributors=[], book=dict(subjects=[supported, unsupported, broad]), description='Wrong description')
        latest = dict(title='Curated title', subtitle='Separate volume', contributors=[dict(name='Curated author')], description='Curated description', book=dict(subjects=[original, manual], languages=['sv'], identifiers=[dict(value='source-id')], publishers=['Source'], year=1900))
        for table, column in [('book', 'hash'), ('external_book', 'entry_key')]:
            with self.subTest(table=table), sqlite3.connect(':memory:') as db:
                db.execute(f'CREATE TABLE {table}({column} TEXT PRIMARY KEY,record TEXT,toc TEXT)')
                db.execute(f'INSERT INTO {table} VALUES(?,?,?)', ('entry', json.dumps(latest), '[{"title":"Chapter"}]'))
                db.commit()
                target = dict(table=table, key='entry')
                result = dict(status='matched', record=returned)
                self.assertEqual(refine.apply_subjects(db, target, result, {'PR4034'}), [supported])
                actual, toc = db.execute(f'SELECT record,toc FROM {table}').fetchone()
                expected = json.loads(json.dumps(latest))
                expected['book']['subjects'].append(supported)
                self.assertEqual(json.loads(actual), expected)
                self.assertEqual(toc, '[{"title":"Chapter"}]')
                self.assertEqual(refine.apply_subjects(db, target, result, {'PR4034'}), [])

    def test_failed_or_missing_match_does_not_write_subjects(self):
        with sqlite3.connect(':memory:') as db:
            db.execute('CREATE TABLE book(hash TEXT PRIMARY KEY,record TEXT)')
            raw = json.dumps(dict(book=dict(subjects=[])))
            db.execute('INSERT INTO book VALUES(?,?)', ('entry', raw))
            db.commit()
            target = dict(table='book', key='entry')
            self.assertEqual(refine.apply_subjects(db, target, dict(status='no_match', record=None), set()), [])
            with self.assertRaisesRegex(RuntimeError, 'Lookup failed'):
                refine.apply_subjects(db, target, dict(status='failed', detail='Lookup failed'), set())
            self.assertEqual(db.execute('SELECT record FROM book').fetchone()[0], raw)


if __name__ == '__main__':
    unittest.main()
