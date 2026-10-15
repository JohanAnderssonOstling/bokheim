import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

import test_deduplicate_catalog as catalog_fixture


SPEC = importlib.util.spec_from_file_location('standardebooks_zips', Path(__file__).parents[1] / 'tools/ingest-standardebooks-zips.py')
importer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(importer)


class StandardEbooksTests(unittest.TestCase):
    def catalog(self):
        fixture = catalog_fixture.DeduplicateTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        return fixture

    def test_imported_edition_replaces_exact_match_while_preserving_other_volumes(self):
        f = self.catalog()
        f.book('old', 1, package_title='A Work')
        f.book('incoming', 1, source='standardebooks.org', package_title='A Work')
        f.book('other-volume', 2)
        dedup, plan = importer.replacement_plan(f.db, f.root, {'incoming'})
        self.assertEqual(len(plan), 1)
        self.assertEqual(plan[0]['kept']['id'], 'incoming')
        self.assertEqual([r['id'] for r in plan[0]['removed']], ['old'])
        dedup.apply(f.db, plan, f.root)
        self.assertEqual(set(r[0] for r in f.db.execute('SELECT hash FROM book')), {'incoming', 'other-volume'})
        self.assertTrue((f.root / 'books/old.epub').exists())
        self.assertIsNotNone(f.db.execute("SELECT 1 FROM book_source WHERE hash='incoming' AND source='gutenberg.org'").fetchone())

    def test_public_domain_library_and_incoming_edition_are_both_kept(self):
        f = self.catalog()
        f.book('preferred', 1, source='publicdomainlibrary.org', package_title='A Work')
        f.book('incoming', 1, source='standardebooks.org', package_title='A Work')
        f.book('unrelated-one', 2, title='Other')
        f.book('unrelated-two', 2, title='Other')
        _, plan = importer.replacement_plan(f.db, f.root, {'incoming'})
        self.assertEqual(plan, [])

    def test_all_incoming_editions_and_preferred_edition_survive_old_duplicate_removal(self):
        f = self.catalog()
        f.book('old', 1, package_title='A Work')
        f.book('preferred', 1, source='publicdomainlibrary.org', package_title='A Work')
        f.book('incoming-one', 1, source='standardebooks.org', package_title='A Work')
        f.book('incoming-two', 1, source='standardebooks.org', package_title='A Work')
        dedup, plan = importer.replacement_plan(f.db, f.root, {'incoming-one', 'incoming-two'})
        self.assertEqual(len(plan), 1)
        self.assertEqual([r['id'] for r in plan[0]['removed']], ['old'])
        retained = {('book', key) for key in ('preferred', 'incoming-one', 'incoming-two')}
        dedup.apply(f.db, plan, f.root, retain_entries=retained)
        self.assertEqual(set(r[0] for r in f.db.execute('SELECT hash FROM book')), {key for _, key in retained})

    def test_apply_rejects_any_incoming_entry_selected_for_removal(self):
        f = self.catalog()
        f.book('old', 1, package_title='A Work')
        f.book('incoming', 1, source='standardebooks.org', package_title='A Work')
        dedup, plan = importer.replacement_plan(f.db, f.root, {'incoming'})
        with self.assertRaisesRegex(ValueError, 'retained catalog entry'):
            dedup.apply(f.db, plan, f.root, retain_entries={('book', 'old')})

    def test_later_missing_book_import_keeps_previous_standardebooks_editions(self):
        f = self.catalog()
        f.book('old', 1, package_title='A Work')
        f.book('previous-standard', 1, source='standardebooks.org', package_title='A Work')
        f.book('new-standard', 1, source='standardebooks.org', package_title='A Work')
        dedup, plan = importer.replacement_plan(f.db, f.root, {'new-standard'})
        self.assertEqual([row['id'] for row in plan[0]['removed']], ['old'])
        dedup.apply(f.db, plan, f.root, retain_entries={('book', 'previous-standard'), ('book', 'new-standard')})
        self.assertEqual(set(row[0] for row in f.db.execute('SELECT hash FROM book')), {'previous-standard', 'new-standard'})

    def test_inventory_reads_nested_epubs_and_deduplicates_overlap_by_content(self):
        content = io.BytesIO()
        with zipfile.ZipFile(content, 'w') as epub:
            epub.writestr('META-INF/container.xml', '<container><rootfiles><rootfile full-path="book.opf"/></rootfiles></container>')
            epub.writestr('book.opf', '<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>A Work</dc:title><dc:creator>An Author</dc:creator><dc:identifier>https://standardebooks.org/ebooks/an-author/a-work</dc:identifier></metadata></package>')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ('se-ebooks-fiction.zip', 'se-ebooks-shorts.zip'):
                with zipfile.ZipFile(root / name, 'w') as archive:
                    archive.writestr('nested/book.epub', content.getvalue())
            entries = importer.inventory(root)
            self.assertEqual(len(entries), 1)
            self.assertTrue(entries[0]['preserve_covers'])
            self.assertEqual(len(entries[0]['source_evidence']['archive_observations']), 2)
            self.assertEqual(len(list(root.iterdir())), 2)

    def test_rate_limit_saves_progress_and_stops_without_retrying_or_requesting_next_book(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / 'audit'
            output.mkdir()
            (output / 'curation-before.sqlite3.gz').touch()
            (output / 'original-covers.json').write_text('{}')
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps([
                {'file': name + '.epub', 'entry_id': name, 'source_evidence': {}}
                for name in ('first', 'second')
            ]))
            args = SimpleNamespace(output=output, manifest=manifest, popularity_only=None,
                                   catalog=root, binary=Path('catalog'))
            response = SimpleNamespace(returncode=1, stdout='', stderr='HTTP status client error (429 Too Many Requests)')
            with patch.object(importer.subprocess, 'run', return_value=response) as request:
                with self.assertRaisesRegex(RuntimeError, 'rate limit reached; progress saved'):
                    importer.run(args)
            self.assertEqual(request.call_count, 1)
            progress = json.loads((output / 'progress.json').read_text())
            self.assertEqual(set(progress), {'first'})
            self.assertEqual(progress['first']['state'], 'failed')


if __name__ == '__main__':
    unittest.main()
