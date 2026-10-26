import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'backfill_audiobook_durations', Path(__file__).parents[1] / 'tools/backfill-audiobook-durations.py')
backfill = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(backfill)


class BackfillAudioDurationsTests(unittest.TestCase):
    def target(self, metadata_url='https://archive.org/download/recording/audio.zip', **metadata):
        return {'key': '12' * 32, 'source': 'loyalbooks.com', 'entry_id': 'book', 'title': 'A book',
                'download_url': 'https://archive.org/download/recording/audio.zip',
                'metadata': json.dumps({'audiobook_download_url': metadata_url, 'rating': 5, **metadata})}

    def plan(self):
        return backfill.make_plan([self.target()], {'recording': (
            {'runtime': '7:43:09'}, {'archive_identifier': 'recording', 'runtime': '7:43:09'})})

    def test_archive_identity_normalizes_delivery_urls_without_using_titles(self):
        for url in ['http://www.archive.org/download//recording/audio.zip',
                    'https://archive.org/compress/recording/formats=64KBPS%20MP3']:
            self.assertEqual(backfill.archive_item(url), 'recording')
        for url in ['https://example.org/download/recording/audio.zip',
                    'https://user:password@archive.org/download/recording/audio.zip',
                    'https://archive.org/download/%2Fother/audio.zip']:
            self.assertIsNone(backfill.archive_item(url))

    def test_runtime_rejects_ambiguous_invalid_and_overflowing_clocks(self):
        self.assertEqual(backfill.duration_ms('7:43:09'), 27_789_000)
        self.assertEqual(backfill.duration_ms('0:00:01.125'), 1125)
        for runtime in ['12:56.25', '12345', '0:60:00', '0:00:60', '0:00:00',
                        '9999999999999999999:00:00', None, ['7:43:09']]:
            self.assertIsNone(backfill.duration_ms(runtime))

    def test_plan_requires_matching_source_and_download_and_keeps_known_duration(self):
        observations = {'recording': ({'runtime': '7:43:09'}, {'archive_identifier': 'recording'})}
        plan = backfill.make_plan([self.target('https://archive.org/download/other/audio.zip')], observations)
        self.assertEqual(plan['updated_recordings'], 0)
        self.assertEqual(len(plan['missing']), 1)
        plan = backfill.make_plan([self.target(audiobook_duration_ms=123_000)], observations)
        self.assertEqual(plan['updates'], [])
        self.assertEqual(len(plan['already_known']), 1)
        self.assertEqual(self.plan()['updates'][0]['duration_ms'], 27_789_000)

    def test_librivox_seconds_verify_short_and_long_clock_notation(self):
        self.assertEqual(backfill.librivox_duration({'totaltime': '51:25', 'totaltimesecs': 3085}), 3_085_000)
        self.assertEqual(backfill.librivox_duration({'totaltime': '7:43:09', 'totaltimesecs': 27789}), 27_789_000)
        for book in [{'totaltime': '51:25', 'totaltimesecs': 3086},
                     {'totaltime': '0:00:00', 'totaltimesecs': 0},
                     {'totaltime': '0:00:01', 'totaltimesecs': True}]:
            self.assertIsNone(backfill.librivox_duration(book))

    def test_same_archive_item_with_a_different_archive_file_is_not_a_match(self):
        observations = {'recording': ({'runtime': '7:43:09'}, {'archive_identifier': 'recording'})}
        plan = backfill.make_plan([self.target('https://archive.org/download/recording/part-one.zip')], observations)
        self.assertEqual(plan['updates'], [])
        self.assertEqual(len(plan['missing']), 1)

    def test_retained_source_page_requires_verified_bytes_and_the_exact_download(self):
        import hashlib
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / 'loyalbooks-book.html'
            raw = b'<a href="http://www.archive.org/download//recording/audio.zip">MP3 Download</a><p>(4:55:36 long)</p>'
            path.write_bytes(raw)
            url = 'https://www.loyalbooks.com/book/book'
            receipt = {'url': url, 'sha256': hashlib.sha256(raw).hexdigest(), 'observed_at': '2026-10-24'}
            path.with_suffix('.http.json').write_text(json.dumps(receipt))
            target = {**self.target(), 'entry_url': url}
            observations = {}
            backfill.retained_source_pages([target], observations, root)
            self.assertEqual(backfill.make_plan([target], observations)['updates'][0]['duration_ms'], 17_736_000)
            wrong = {**target, 'download_url': 'https://archive.org/download/recording/other.zip'}
            observations = {}
            backfill.retained_source_pages([wrong], observations, root)
            self.assertEqual(observations, {})
            path.write_bytes(raw + b'changed')
            with self.assertRaises(ValueError):
                backfill.retained_source_pages([target], {}, root)

    def test_staged_curation_changes_only_duration_evidence_and_keeps_input_unchanged(self):
        with tempfile.TemporaryDirectory() as temporary:
            source, destination = Path(temporary) / 'before.sqlite3', Path(temporary) / 'after.sqlite3'
            target = self.target()
            with sqlite3.connect(source) as db:
                db.execute('CREATE TABLE source_entry(source,entry_id,external_entry_key,metadata)')
                db.execute('INSERT INTO source_entry VALUES(?,?,?,?)',
                           (target['source'], target['entry_id'], target['key'], target['metadata']))
            before = source.read_bytes()
            backfill.stage_curation(source, destination, self.plan())
            self.assertEqual(source.read_bytes(), before)
            with sqlite3.connect(destination) as db:
                metadata = json.loads(db.execute('SELECT metadata FROM source_entry').fetchone()[0])
            self.assertEqual(metadata.pop('audiobook_duration_ms'), 27_789_000)
            self.assertEqual(metadata.pop('audiobook_duration_evidence')['archive_identifier'], 'recording')
            self.assertEqual(metadata, json.loads(target['metadata']))

    def snapshot(self, path, archive='recording'):
        with sqlite3.connect(path) as db:
            db.execute('CREATE TABLE snapshot(singleton PRIMARY KEY,format_version,release_id)')
            db.execute('INSERT INTO snapshot VALUES(1,8,279)')
            db.execute('CREATE TABLE book(row_id INTEGER PRIMARY KEY,key BLOB UNIQUE,external,download_url,format,title)')
            db.execute('INSERT INTO book VALUES(1,?,1,?,16,?)',
                       (bytes.fromhex(self.target()['key']), f'https://archive.org/download/{archive}/audio.zip', 'A book'))
            db.execute('CREATE TABLE catalog_browse_search(row_id,book_row_id)')
            db.execute('CREATE VIEW catalog_browse_book AS SELECT * FROM catalog_browse_search')

    def test_staged_snapshot_preserves_records_and_adds_duration_in_a_new_release(self):
        with tempfile.TemporaryDirectory() as temporary:
            source, destination = Path(temporary) / 'before.sqlite3', Path(temporary) / 'after.sqlite3'
            self.snapshot(source)
            before = source.read_bytes()
            receipt = backfill.stage_snapshot(source, destination, self.plan(), 280)
            self.assertEqual(receipt['previous_release_id'], 279)
            self.assertEqual(source.read_bytes(), before)
            with sqlite3.connect(source) as original, sqlite3.connect(destination) as staged:
                self.assertEqual(original.execute('SELECT * FROM book').fetchall(), staged.execute('SELECT * FROM book').fetchall())
                self.assertEqual(staged.execute('SELECT * FROM book_audiobook_duration').fetchall(), [(1, 27_789_000)])
                self.assertEqual(staged.execute('SELECT release_id FROM snapshot').fetchone(), (280,))
                self.assertEqual(staged.execute("SELECT name FROM sqlite_schema WHERE name GLOB 'catalog_*'").fetchall(), [])
            with self.assertRaises(ValueError):
                backfill.stage_snapshot(source, destination, self.plan(), 280)

    def test_staging_rejects_different_recordings_and_non_increasing_releases(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'before.sqlite3'
            self.snapshot(source, archive='different-recording')
            for release in [279, 280]:
                with self.assertRaises(ValueError):
                    backfill.stage_snapshot(source, root / f'rejected-{release}.sqlite3', self.plan(), release)


if __name__ == '__main__':
    unittest.main()
