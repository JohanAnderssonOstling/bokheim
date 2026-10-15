import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location('standardebooks_popularity', Path(__file__).parents[1] / 'tools/standardebooks-popularity.py')
popularity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(popularity)


class PopularityTests(unittest.TestCase):
    def pages(self, root, duplicate=False):
        controls = '<select name="sort"><option value="popularity" selected="selected"/></select><select name="per-page"><option value="48" selected="selected"/></select>'
        pagination = '<a href="/ebooks?sort=popularity&amp;per-page=48&amp;page=2">2</a>'
        for page, numbers in [(1, range(1, 49)), (2, [1 if duplicate else 49])]:
            books = ''.join(f'<li typeof="schema:Book" about="/ebooks/author/book-{number}"><p><a><span property="schema:name">Book {number}</span></a></p><p property="schema:author"><a><span property="schema:name">An Author</span></a></p></li>' for number in numbers)
            (root / f'page-{page:02}.xhtml').write_text('<html>' + controls + pagination + books + '</html>')

    def test_complete_48_book_pagination_produces_one_contiguous_ranking(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.pages(root)
            result = popularity.ranking(root)
            self.assertEqual(result['total'], 49)
            self.assertEqual([entry['rank'] for entry in result['entries']], list(range(1, 50)))
            self.assertEqual([page['books'] for page in result['pages']], [48, 1])
            self.assertEqual(result['entries'][-1]['entry_id'], 'https://standardebooks.org/ebooks/author/book-49')
            self.assertEqual(result['entries'][-1]['title'], 'Book 49')
            self.assertEqual(result['entries'][-1]['authors'], ['An Author'])

    def test_duplicate_or_missing_pages_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.pages(root, duplicate=True)
            with self.assertRaisesRegex(ValueError, 'multiple popularity pages'):
                popularity.ranking(root)
            (root / 'page-02.xhtml').unlink()
            with self.assertRaisesRegex(ValueError, 'complete pagination'):
                popularity.ranking(root)

    def test_ranks_join_on_source_identity_and_preserve_other_evidence(self):
        with tempfile.TemporaryDirectory() as temporary, sqlite3.connect(':memory:') as db:
            root = Path(temporary)
            self.pages(root)
            manifest = popularity.ranking(root)
            identity = manifest['entries'][20]['entry_id']
            db.execute('CREATE TABLE source_entry(source,entry_id,book_hash,metadata)')
            original = {'cover': {'source_url': 'https://example.org/chosen.jpg'}, 'sha256': 'archive-epub'}
            db.execute('INSERT INTO source_entry VALUES(?,?,?,?)', ['standardebooks.org', identity, 'book-hash', json.dumps(original)])
            db.execute('INSERT INTO source_entry VALUES(?,?,?,?)', ['gutenberg.org', '21', 'other-hash', '{"downloads":7}'])
            self.assertEqual(popularity.apply(db, manifest), 1)
            evidence = json.loads(db.execute("SELECT metadata FROM source_entry WHERE source='standardebooks.org'").fetchone()[0])
            self.assertEqual(evidence['cover'], original['cover'])
            self.assertEqual(evidence['sha256'], original['sha256'])
            self.assertEqual(evidence['popularity']['rank'], 21)
            self.assertEqual(evidence['popularity']['kind'], 'ordinal')
            self.assertEqual(db.execute("SELECT metadata FROM source_entry WHERE source='gutenberg.org'").fetchone()[0], '{"downloads":7}')


if __name__ == '__main__':
    unittest.main()
