import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
import zipfile

from test_merge_epubs import fixture

SPEC = importlib.util.spec_from_file_location('deduplicate_catalog', Path(__file__).parents[1] / 'tools/deduplicate-catalog.py')
dedup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dedup)


class DeduplicateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'books').mkdir()
        (self.root / 'covers').mkdir()
        self.db = sqlite3.connect(':memory:')
        self.addCleanup(self.db.close)
        self.db.executescript('''
            PRAGMA foreign_keys=ON;
            CREATE TABLE book(hash TEXT PRIMARY KEY,title,subtitle,author,record,toc);
            CREATE TABLE book_source(hash REFERENCES book(hash) ON DELETE CASCADE,source,entry_id,entry_url,file_url,PRIMARY KEY(hash,source));
            CREATE TABLE external_book(entry_key TEXT PRIMARY KEY,source,format,title,subtitle,author,record,toc);
            CREATE TABLE source_entry(book_hash REFERENCES book(hash) ON DELETE SET NULL,external_entry_key,metadata);
        ''')

    def book(self, key, volume, source='gutenberg.org', package_title=None, language='en', title='A Work'):
        path = self.root / 'books' / (key + '.epub')
        files = fixture(path, volume)
        if package_title:
            with zipfile.ZipFile(path) as archive:
                files = {item.filename: archive.read(item.filename) for item in archive.infolist()}
            files['OEBPS/content.opf'] = files['OEBPS/content.opf'].replace(f'A Work, Volume {volume}'.encode(), package_title.encode())
            with zipfile.ZipFile(path, 'w') as archive:
                for name, data in files.items(): archive.writestr(name, data)
        record = json.dumps({'book': {'languages': [language], 'subjects': []}, 'description': ''})
        self.db.execute('INSERT INTO book VALUES(?,?,?,?,?,?)', [key, title, None, 'An Author', record, '[]'])
        self.db.execute('INSERT INTO book_source VALUES(?,?,?,?,?)', [key, source, key, None, None])
        self.db.execute('INSERT INTO source_entry VALUES(?,NULL,?)', [key, json.dumps({'source_entry': key, 'downloads': volume})])

    def test_same_enriched_title_preserves_other_volume_and_prefers_pdl_within_one_volume(self):
        self.book('pg-one', 1)
        self.book('pg-two', 2)
        self.book('pdl-one', 1, dedup.PREFERRED_SOURCE, language='fr')
        plan = dedup.plan(self.db, self.root)
        self.assertEqual(len(plan), 1)
        self.assertEqual(plan[0]['kept']['id'], 'pdl-one')
        self.assertEqual([r['id'] for r in plan[0]['removed']], ['pg-one'])
        # A saved and reloaded review must still apply the exact same scope.
        dedup.apply(self.db, json.loads(json.dumps(plan)), self.root)
        self.assertEqual(set(r[0] for r in self.db.execute('SELECT hash FROM book')), {'pdl-one', 'pg-two'})
        self.assertEqual(self.db.execute("SELECT book_hash FROM source_entry WHERE json_extract(metadata,'$.source_entry')='pg-two'").fetchone()[0], 'pg-two')
        self.assertTrue((self.root / 'books/pg-one.epub').exists())

    def test_review_cannot_remove_a_different_volume(self):
        self.book('pg-one', 1)
        self.book('pdl-one', 1, dedup.PREFERRED_SOURCE)
        self.book('pg-two', 2)
        plan = dedup.plan(self.db, self.root)
        columns = ['kind', 'id', 'format', 'title', 'subtitle', 'author', 'record', 'toc', 'ordinal']
        row = dict(zip(columns, next(r for r in self.db.execute(dedup.ENTRIES) if r[1] == 'pg-two')))
        row['volume_scope'] = dedup.volume_scope(row, self.root)
        plan[0]['removed'].append(row)
        with self.assertRaisesRegex(ValueError, 'Different source volumes'):
            dedup.apply(self.db, plan, self.root)
        self.assertEqual(self.db.execute('SELECT count(*) FROM book').fetchone()[0], 3)

    def test_distinct_issues_and_slices_of_one_volume_are_preserved(self):
        self.book('issue-one', 1, package_title='A Work, Vol. I, No.1')
        self.book('issue-two', 1, package_title='A Work, Volume 1, No. 2')
        self.book('slice-one', 1, package_title='A Work, Volume 1, slice 1 of 2')
        self.book('slice-two', 1, package_title='A Work, Volume 1, slice 2 of 2')
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_whole_books_keep_existing_preferred_source_policy(self):
        self.book('pg', 1, package_title='A Work')
        self.book('pdl', 1, dedup.PREFERRED_SOURCE, package_title='A Work', language='fr')
        plan = dedup.plan(self.db, self.root)
        self.assertEqual(plan[0]['kept']['id'], 'pdl')
        dedup.apply(self.db, plan, self.root)
        self.assertEqual(self.db.execute('SELECT hash FROM book').fetchall(), [('pdl',)])

    def test_repeated_volume_word_preserves_bundled_scope(self):
        self.book('single', 1, package_title='A Work, Volume I')
        self.book('bundled', 1, package_title='A Work, Volume I and Volume II')
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_turpin_selection_is_kept_separate_from_complete_emerson_essays(self):
        self.book('complete', 1, package_title='Essays', title='Essays')
        self.book('selection', 1, package_title='Essays by Ralph Waldo Emerson', title='Essays')
        path = self.root / 'books/selection.epub'
        with zipfile.ZipFile(path) as archive:
            files = {name: archive.read(name) for name in archive.namelist()}
        files['OEBPS/content.opf'] = files['OEBPS/content.opf'].replace(
            b'</metadata>', b'<dc:contributor>Edna Henry Lee Turpin</dc:contributor></metadata>')
        with zipfile.ZipFile(path, 'w') as archive:
            for name, data in files.items(): archive.writestr(name, data)
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_short_volume_markers_cannot_collapse_into_one_enriched_title(self):
        for i in range(1, 5):
            self.book(f'plotinos-{i}', i, package_title=f'Plotinos: Complete Works, v. {i}', title='Plotinos')
        self.assertEqual(dedup.plan(self.db, self.root), [])
        self.assertEqual(self.db.execute('SELECT count(*) FROM book').fetchone()[0], 4)

    def test_book_ranges_and_single_books_with_same_display_title_are_preserved(self):
        for i, package_title in enumerate(['A Work, Books 01 to 08', 'A Work, Books 09 to 26',
                                          'A Work, Book 37', 'A Work, Books 37 to the End'], 1):
            self.book(f'book-{i}', i, package_title=package_title)
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_implicit_positions_and_distinct_named_essay_collections_are_preserved(self):
        for i, title in enumerate(['A Work (1 de 2)', 'A Work (2 de 2)',
                                  'The Essays of Arthur Schopenhauer; The Art of Literature',
                                  'The Essays of Arthur Schopenhauer: Counsels and Maxims'], 1):
            self.book(f'source-{i}', i, package_title=title)
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_dutch_parts_with_one_display_title_are_preserved(self):
        self.book('dutch-one', 1, package_title='Een werk, Deel 1 van 2', language='nl')
        self.book('dutch-two', 2, package_title='Een werk, Deel 2 van 2', language='nl')
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_swedish_ordinal_and_bare_parts_with_shared_display_title_are_preserved(self):
        for i, title in enumerate(['Ett verk. Första delen', 'Ett verk. Andra delen',
                                  'Ett verk. I', 'Ett verk. II', 'Ett verk VI:1', 'Ett verk VI:2'], 1):
            key = f'swedish-{i}'
            self.book(key, i, package_title=title)
            path = self.root / 'books' / (key + '.epub')
            with zipfile.ZipFile(path) as archive:
                files = {name: archive.read(name) for name in archive.namelist()}
            files['OEBPS/content.opf'] = files['OEBPS/content.opf'].replace(b'<dc:language>en</dc:language>', b'<dc:language>sv</dc:language>')
            with zipfile.ZipFile(path, 'w') as archive:
                for name, data in files.items():
                    archive.writestr(name, data)
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_slash_positions_with_one_display_title_are_preserved(self):
        for i in range(1, 4):
            self.book(f'vandal-{i}', i, package_title=f'Une histoire ({i}/3)', language='fr')
        self.assertEqual(dedup.plan(self.db, self.root), [])

    def test_titlepage_volume_numbers_preserve_books_with_bare_package_titles(self):
        for i in (1, 2):
            self.book(f'bare-{i}', i, package_title='Celtic Scotland', title='Celtic Scotland')
            path = self.root / 'books' / f'bare-{i}.epub'
            with zipfile.ZipFile(path) as archive:
                files = {name: archive.read(name) for name in archive.namelist()}
            files['OEBPS/chapter.xhtml'] = files['OEBPS/chapter.xhtml'].replace(
                b'<body>', f'<body><p>Volume {i}.</p>'.encode())
            with zipfile.ZipFile(path, 'w') as archive:
                for name, data in files.items(): archive.writestr(name, data)
        self.assertEqual(dedup.plan(self.db, self.root), [])


if __name__ == '__main__': unittest.main()
