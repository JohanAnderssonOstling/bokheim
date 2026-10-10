import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('migrate_provider', Path(__file__).parent / 'tools/migrate-provider-caches.py')
migration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(migration)


class MigrationTests(unittest.TestCase):
    def test_completed_results_and_quota_move_without_keys_or_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            old = root / 'old.sqlite'
            db = sqlite3.connect(old)
            db.executescript('''CREATE TABLE loc_api_result(isbn13 TEXT PRIMARY KEY,result TEXT,checked_at INTEGER);
                CREATE TABLE librarything_isbn_lcc(isbn13 TEXT PRIMARY KEY,result TEXT,checked_at INTEGER);
                CREATE TABLE librarything_requests(id INTEGER PRIMARY KEY,day INTEGER,count INTEGER,last_at INTEGER);
                INSERT INTO librarything_requests VALUES(1,100,999,8640010);''')
            db.execute('INSERT INTO loc_api_result VALUES(?,?,?)', ('9780306406157', '"NoMatch"', 10))
            db.execute('INSERT INTO librarything_isbn_lcc VALUES(?,?,?)', ('9780306406157', '{"work_id":"7","codes":["QA76"],"ddc":["005.1"]}', 20))
            db.commit()
            db.close()
            out = root / 'provider.sqlite'
            self.assertEqual(migration.migrate(old, old, out), 2)
            db = sqlite3.connect(out)
            self.assertEqual(db.execute("SELECT fetched_at_ms,status FROM provider_result WHERE provider='loc'").fetchone(), (10000, 'no_match'))
            raw = db.execute("SELECT classifications FROM provider_result WHERE provider='librarything'").fetchone()[0]
            self.assertIsInstance(raw, bytes)
            codes = migration.json.loads(raw)
            self.assertEqual([c['scheme'] for c in codes], ['library_of_congress', 'dewey_decimal'])
            self.assertEqual(codes[0]['evidence'][0]['method'], 'librarything_isbn:7')
            self.assertEqual(db.execute('SELECT day,used FROM provider_quota').fetchone(), (100, 999))
            self.assertEqual(db.execute('SELECT next_ms FROM provider_rate').fetchone(), (8640012000,))
            db.close()
            before = out.read_bytes()
            with self.assertRaises(ValueError):
                migration.migrate(old, old, out)
            self.assertEqual(out.read_bytes(), before)
