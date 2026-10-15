import importlib.util
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from test_merge_epubs import fixture, merge


SPEC = importlib.util.spec_from_file_location('import_merged_epub', Path(__file__).parents[1] / 'tools/import-merged-epub.py')
importer = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = importer
SPEC.loader.exec_module(importer)


class ImportTests(unittest.TestCase):
    def test_failed_backup_leaves_no_reusable_partial_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'backup.sqlite3'

            class FailingBackup:
                def backup(self, destination):
                    destination.execute('CREATE TABLE partial(value)')
                    destination.commit()
                    raise sqlite3.OperationalError('database or disk is full')

            with self.assertRaises(sqlite3.OperationalError):
                importer.backup_catalog(FailingBackup(), path)
            self.assertFalse(path.exists())
            self.assertEqual(list(Path(temporary).iterdir()), [])
            with sqlite3.connect(':memory:') as source:
                source.execute('CREATE TABLE book(hash,record)')
                source.execute('INSERT INTO book VALUES(?,?)', ['original', '{}'])
                source.commit()
                importer.backup_catalog(source, path)
            with sqlite3.connect(path) as backup:
                self.assertEqual(backup.execute('SELECT hash FROM book').fetchall(), [('original',)])

    def test_existing_empty_backup_is_rejected_and_retained(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'backup.sqlite3'
            with sqlite3.connect(path) as backup:
                self.assertEqual(backup.execute('PRAGMA integrity_check').fetchone()[0], 'ok')
            with sqlite3.connect(':memory:') as source:
                with self.assertRaisesRegex(ValueError, 'incomplete or unreadable'):
                    importer.backup_catalog(source, path)
            self.assertTrue(path.exists())

    def test_combined_description_preserves_source_text_instead_of_generic_placeholder(self):
        book = {'identifiers': [], 'subjects': [], 'publishers': [], 'languages': ['en'], 'year': 1900}
        components = [{'label': 'Volume I', 'record': {'description': 'First original description.', 'contributors': [], 'book': book}},
                      {'label': 'Volume II', 'record': {'description': 'Second original description.', 'contributors': [], 'book': book}}]
        record = importer.combined_record({'book': {}, 'description': 'An enriched generic description.'}, components, 'A Work')
        self.assertEqual(record['description'], 'Volume I\nFirst original description.\n\nVolume II\nSecond original description.')
        self.assertEqual(components[0]['record']['description'], 'First original description.')

    def test_combined_record_retains_later_contributors_with_their_roles(self):
        author = {'name': 'An Author', 'contributor_id': 'first-id', 'role': list(b'aut')}
        repeated = {**author, 'contributor_id': 'other-source-id'}
        translator = {'name': 'A Translator', 'contributor_id': 'translator-id', 'role': list(b'trl')}
        editor = {**translator, 'role': list(b'edt')}
        book = {'identifiers': [], 'subjects': [], 'publishers': [], 'languages': ['en'], 'year': 1900}
        first = {'contributors': [author], 'book': book}
        second = {'contributors': [repeated, translator, editor], 'book': book}
        record = importer.combined_record({'book': {}}, [{'record': first}, {'record': second}], 'A Work')
        self.assertEqual(record['contributors'], [author, translator, editor])
        self.assertEqual(first['contributors'], [author])

    def test_reimport_of_replaced_source_identity_is_resumable(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            catalog = root / 'catalog'
            (catalog / 'books').mkdir(parents=True)
            (catalog / 'covers').mkdir()
            paths = [catalog / 'books/first.epub', catalog / 'books/second.epub']
            for index, path in enumerate(paths, 1):
                fixture(path, index)
            report = merge.merge_epubs(paths, root / 'a-work.epub', 'A Work')
            report_path = root / 'merge-report.json'
            report_path.write_text(json.dumps(report))
            record = {'title': 'A Work', 'subtitle': None, 'description': '',
                      'contributors': [{'name': 'An Author', 'contributor_id': 'author-id', 'role': list(b'aut')}],
                      'book': {'identifiers': [], 'subjects': [], 'publishers': [], 'languages': ['en'], 'year': 2026}}
            db_path = catalog / 'curation.sqlite3'
            with sqlite3.connect(db_path) as db:
                db.executescript('''CREATE TABLE book(hash TEXT PRIMARY KEY,title,subtitle,author,record,toc);
                    CREATE TABLE book_source(hash,source,entry_id,entry_url,file_url);
                    CREATE TABLE source_entry(source,entry_id,entry_url,book_hash,external_entry_key,metadata,
                                              PRIMARY KEY(source,entry_id));''')
                for path in paths:
                    db.execute('INSERT INTO book VALUES(?,?,?,?,?,?)', [path.stem, record['title'], None, 'An Author', json.dumps(record), '[]'])
                    db.execute('INSERT INTO source_entry VALUES(?,?,?,?,?,?)', ['test-source', path.stem, 'https://example.com/' + path.stem, path.stem, None, '{}'])
                db.execute('INSERT INTO source_entry VALUES(?,?,?,?,?,?)', ['bokheim:combined-volumes', report['identifier'], 'https://example.com/old', None, None, '{}'])

            def inspect(command, **kwargs):
                stage = Path(command[1])
                with sqlite3.connect(stage / 'curation.sqlite3') as db:
                    db.execute('CREATE TABLE book(hash,record,toc)')
                    db.execute('INSERT INTO book VALUES(?,?,?)', ['combined-hash', json.dumps(record), '[]'])
                return SimpleNamespace(returncode=0, stdout='', stderr='')

            args = SimpleNamespace(catalog=catalog, binary=Path('/unused/catalog'), report=report_path,
                                   replace_merged=None, backup=root / 'shared-backup.sqlite3')
            with patch.object(importer.subprocess, 'run', side_effect=inspect):
                receipt = importer.run(args)
            self.assertEqual(receipt['state'], 'imported')
            self.assertTrue(args.backup.exists())
            with sqlite3.connect(db_path) as db:
                self.assertEqual(db.execute('SELECT book_hash FROM source_entry WHERE source=? AND entry_id=?',
                                           ['bokheim:combined-volumes', report['identifier']]).fetchone()[0], 'combined-hash')
                self.assertEqual(db.execute('SELECT count(*) FROM book').fetchone()[0], 3)
            with patch.object(importer.subprocess, 'run', side_effect=AssertionError('Retry must not reinspect')):
                second = importer.run(args)
            self.assertEqual(second['state'], 'already_imported')

    def test_only_reviewed_additional_inspector_credits_are_retained(self):
        book = {'identifiers': [], 'subjects': [], 'publishers': [], 'languages': ['en'], 'year': 1900}
        credit = {'name': 'An Essay Writer', 'role': list(b'ctb'), 'contributor_id': 'writer-id'}
        unrelated = {'name': 'Unreviewed Enrichment', 'role': list(b'aut'), 'contributor_id': 'unrelated-id'}
        inspected = {'book': {}, 'contributors': [credit, unrelated]}
        components = [{'record': {'book': book, 'contributors': []}}]
        record = importer.combined_record(inspected, components, 'A Work',
                                         [{'name': 'An Essay Writer', 'role': 'ctb'}])
        self.assertEqual(record['contributors'], [credit])
        with self.assertRaisesRegex(ValueError, 'not retained by the EPUB inspector'):
            importer.combined_record(inspected, components, 'A Work', [{'name': 'Missing', 'role': 'ctb'}])

    def test_archived_components_require_explicit_same_source_replacement(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            catalog = root / 'catalog'
            (catalog / 'books').mkdir(parents=True)
            (catalog / 'covers').mkdir()
            paths = [catalog / 'books/first.epub', catalog / 'books/second.epub']
            for index, path in enumerate(paths, 1):
                fixture(path, index)
            record = {'title': 'A Work', 'subtitle': None, 'description': 'Original summary.',
                      'contributors': [{'name': 'An Author', 'contributor_id': 'id', 'role': list(b'aut')}],
                      'book': {'identifiers': [], 'subjects': [], 'publishers': [], 'languages': ['en'], 'year': 1900}}
            first = merge.merge_epubs(paths, root / 'old.epub', 'A Work')
            components = [{'hash': path.stem, 'sha256': v['sha256'], 'label': v['label'], 'record': record,
                           'source_entries': [{'source': 'original', 'entry_id': path.stem,
                                               'entry_url': 'https://example.com/' + path.stem, 'metadata': {}}]}
                          for path, v in zip(paths, first['volumes'])]
            prior = {'kind': 'combined_volumes', 'components': components,
                     'original_catalog_entries_retained': False, 'originals_archive': '/retained/archive'}
            db_path = catalog / 'curation.sqlite3'
            with sqlite3.connect(db_path) as db:
                db.executescript('''CREATE TABLE book(hash PRIMARY KEY,title,subtitle,author,record,toc);
                    CREATE TABLE book_source(hash,source,entry_id,entry_url,file_url);
                    CREATE TABLE source_entry(source,entry_id,entry_url,book_hash,external_entry_key,metadata,
                                              PRIMARY KEY(source,entry_id));''')
                db.execute('INSERT INTO book VALUES(?,?,?,?,?,?)', ['old-hash', 'A Work', None, 'An Author', json.dumps(record), '[]'])
                db.execute('INSERT INTO book_source VALUES(?,?,?,?,?)', ['old-hash', 'bokheim:combined-volumes', first['identifier'], 'https://example.com/first', None])
                db.execute('INSERT INTO source_entry VALUES(?,?,?,?,?,?)', ['bokheim:combined-volumes', first['identifier'], 'https://example.com/first', 'old-hash', None, json.dumps(prior)])
            report = merge.merge_epubs(paths, root / 'new.epub', 'A Work', description='Original summary.\n\nOriginal mirror summary.')
            report_path = root / 'report.json'
            report_path.write_text(json.dumps(report))
            args = SimpleNamespace(catalog=catalog, binary=Path('/unused'), report=report_path,
                                   replace_merged=None, backup=root / 'backup.sqlite3')
            with self.assertRaisesRegex(ValueError, 'no catalog record'):
                importer.run(args)
            args.replace_merged = 'old-hash'
            corrupted = {**prior, 'components': [{**components[0], 'sha256': 'wrong'}, components[1]]}
            with sqlite3.connect(db_path) as db:
                db.execute('UPDATE source_entry SET metadata=?', [json.dumps(corrupted)])
            with self.assertRaisesRegex(ValueError, 'same ordered source hashes and bytes'):
                importer.run(args)
            with sqlite3.connect(db_path) as db:
                db.execute('UPDATE source_entry SET metadata=?', [json.dumps(prior)])

            def inspect(command, **kwargs):
                with sqlite3.connect(Path(command[1]) / 'curation.sqlite3') as db:
                    db.execute('CREATE TABLE book(hash,record,toc)')
                    db.execute('INSERT INTO book VALUES(?,?,?)', ['new-hash', json.dumps(record), '[]'])
                return SimpleNamespace(returncode=0, stdout='', stderr='')

            with patch.object(importer.subprocess, 'run', side_effect=inspect):
                receipt = importer.run(args)
            self.assertEqual(receipt['replaced_generated_hash'], 'old-hash')
            with sqlite3.connect(db_path) as db:
                self.assertIsNone(db.execute('SELECT 1 FROM book WHERE hash=?', ['old-hash']).fetchone())
                retained = json.loads(db.execute('SELECT metadata FROM source_entry WHERE book_hash=?', ['new-hash']).fetchone()[0])
                self.assertEqual(retained['components'], components)
                self.assertEqual(retained['originals_archive'], '/retained/archive')
                self.assertFalse(retained['original_catalog_entries_retained'])
                updated = json.loads(db.execute('SELECT record FROM book WHERE hash=?', ['new-hash']).fetchone()[0])
                self.assertEqual(updated['description'], report['description'])


if __name__ == '__main__':
    unittest.main()
