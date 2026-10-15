import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location('catalog_duplicates', Path(__file__).parents[1] / 'tools/deduplicate-catalog.py')
duplicates = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(duplicates)


class DuplicatePreferenceTests(unittest.TestCase):
    def test_reviewed_wealth_of_nations_alias_keeps_formats_volumes_and_other_authors_separate(self):
        def key(title, author='Adam Smith', format_code=1):
            return duplicates.match_key(dict(title=title, author=author, format=format_code))
        short_title = 'The Wealth of Nations'
        long_title = 'An Inquiry into the Nature and Causes of the Wealth of Nations'
        self.assertEqual(key(short_title), key(long_title))
        self.assertNotEqual(key(short_title), key(long_title, format_code=16))
        self.assertNotEqual(key(short_title), key('The Wealth of Nations, Book 4'))
        self.assertNotEqual(key(short_title, author='Another author'), key(long_title, author='Another author'))

    def test_public_domain_library_survives_and_other_source_provenance_is_retained(self):
        with tempfile.TemporaryDirectory() as directory, sqlite3.connect(':memory:') as db:
            root = Path(directory)
            (root / 'books').mkdir()
            db.executescript('''
                PRAGMA foreign_keys=ON;
                CREATE TABLE book(hash TEXT PRIMARY KEY,title TEXT,subtitle TEXT,author TEXT,record TEXT,toc TEXT);
                CREATE TABLE external_book(entry_key TEXT PRIMARY KEY,format INTEGER,title TEXT,subtitle TEXT,author TEXT,record TEXT,toc TEXT,source TEXT);
                CREATE TABLE book_source(hash TEXT REFERENCES book(hash) ON DELETE CASCADE,source TEXT,entry_id TEXT,entry_url TEXT,file_url TEXT,PRIMARY KEY(hash,source));
                CREATE TABLE source_entry(source TEXT,entry_id TEXT,book_hash TEXT REFERENCES book(hash) ON DELETE SET NULL,external_entry_key TEXT REFERENCES external_book(entry_key) ON DELETE SET NULL,metadata TEXT,PRIMARY KEY(source,entry_id));
            ''')
            for key, language, subtitle, toc, description, source in [
                ('other', 'en', None, '[1,2,3,4]', 'Rich description', 'gutenberg.org'),
                ('preferred', 'en-GB', 'An edition', '[]', '', 'publicdomainlibrary.org'),
            ]:
                record = json.dumps(dict(book=dict(languages=[language], subjects=[]), description=description))
                db.execute('INSERT INTO book VALUES(?,?,?,?,?,?)', (key, 'The Prince', subtitle, 'Author', record, toc))
                db.execute('INSERT INTO book_source VALUES(?,?,?,?,?)', (key, source, key, 'page', 'download'))
                db.execute('INSERT INTO source_entry VALUES(?,?,?,?,?)', (source, key, key, None, '{"original":true}'))
                (root / 'books' / (key + '.epub')).write_bytes(b'retained')
            # Similar spelling and a different format must stay separate.
            db.execute("INSERT INTO book SELECT 'case-variant','the Prince',subtitle,author,record,toc FROM book WHERE hash='other'")
            db.execute("INSERT INTO external_book SELECT 'pdf',2,title,subtitle,author,record,toc,'publicdomainlibrary.org' FROM book WHERE hash='other'")
            plan = duplicates.plan(db, root)
            self.assertEqual(len(plan), 1)
            self.assertEqual(plan[0]['kept']['id'], 'preferred')
            preferred_record = db.execute("SELECT record,toc FROM book WHERE hash='preferred'").fetchone()
            # A manually reviewed plan cannot override the source preference.
            invalid = dict(plan[0], kept=plan[0]['removed'][0], removed=[plan[0]['kept']])
            with self.assertRaisesRegex(ValueError, 'Public Domain Library'):
                duplicates.apply(db, [invalid])
            # Nor may it remove an entry just because another format exists.
            pdf = dict(plan[0]['kept'], kind='external_book', id='pdf', format=2)
            pdf['record'] = db.execute("SELECT record FROM external_book WHERE entry_key='pdf'").fetchone()[0]
            with self.assertRaisesRegex(ValueError, 'different media formats'):
                duplicates.apply(db, [dict(plan[0], removed=[pdf])])
            duplicates.apply(db, plan)
            self.assertEqual(db.execute('SELECT hash FROM book ORDER BY hash').fetchall(), [('case-variant',), ('preferred',)])
            self.assertEqual(db.execute('SELECT entry_key FROM external_book').fetchall(), [('pdf',)])
            self.assertEqual(db.execute("SELECT record,toc FROM book WHERE hash='preferred'").fetchone(), preferred_record)
            self.assertEqual(db.execute('SELECT source FROM book_source ORDER BY source').fetchall(), [('gutenberg.org',), ('publicdomainlibrary.org',)])
            self.assertEqual(db.execute('SELECT book_hash,metadata FROM source_entry').fetchall(), [('preferred', '{"original":true}'), ('preferred', '{"original":true}')])
            self.assertTrue((root / 'books/other.epub').exists())


if __name__ == '__main__':
    unittest.main()
