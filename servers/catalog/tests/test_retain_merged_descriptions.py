import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location('retained_descriptions', Path(__file__).parents[1] / 'tools/retain-merged-descriptions.py')
TOOL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TOOL)


class RetainedDescriptions(unittest.TestCase):
    def test_page_keeps_hidden_continuation_and_attribution_without_controls(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / '1.html'
            path.write_text('<div class="summary-text-container">First sentence.<span class="readmoredots">...</span><span>Hidden ending. (This is an automatically generated summary.)</span><label>Read more</label></div>')
            text, evidence = TOOL.page_description(path)
            self.assertEqual(text, 'First sentence. Hidden ending. (This is an automatically generated summary.)')
            self.assertEqual(evidence['page_file'], str(path))
            self.assertEqual(len(evidence['page_sha256']), 64)

    def test_restore_preserves_other_fields_components_and_curated_description(self):
        with sqlite3.connect(':memory:') as db:
            db.executescript('CREATE TABLE book(hash TEXT,record TEXT); CREATE TABLE source_entry(source TEXT,entry_id TEXT,book_hash TEXT,metadata TEXT);')
            components = [{'hash': 'one', 'label': 'Volume 1', 'record': {'description': 'First description.'}, 'source_entries': []},
                          {'hash': 'two', 'label': 'Volume 2', 'record': {'description': 'Second description.'}, 'source_entries': []}]
            for identity, description in [('generated', 'Combined edition of 2 volumes.'), ('curated', 'Reviewed synopsis.')]:
                db.execute('INSERT INTO book VALUES(?,?)', [identity, json.dumps({'title': identity, 'description': description, 'book': {'year': 1888}})])
                db.execute('INSERT INTO source_entry VALUES(?,?,?,?)', ['bokheim:combined-volumes', identity, identity, json.dumps({'components': components, 'originals_archive': 'unchanged'})])
            db.commit()
            result = TOOL.restore(db, {})
            self.assertEqual(result['updated'], 1)
            record = json.loads(db.execute("SELECT record FROM book WHERE hash='generated'").fetchone()[0])
            self.assertEqual(record['description'], 'Volume 1\nFirst description.\n\nVolume 2\nSecond description.')
            self.assertEqual(record['book'], {'year': 1888})
            evidence = json.loads(db.execute("SELECT metadata FROM source_entry WHERE entry_id='generated'").fetchone()[0])
            self.assertEqual(evidence['components'], components)
            self.assertEqual(evidence['originals_archive'], 'unchanged')
            self.assertEqual(json.loads(db.execute("SELECT record FROM book WHERE hash='curated'").fetchone()[0])['description'], 'Reviewed synopsis.')
            self.assertEqual(TOOL.restore(db, {})['updated'], 0)

    def test_recovery_uses_page_for_empty_component_without_changing_original_record(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / '1.html'
            path.write_text('<table class="bibrec"><tr><th>Summary</th><td>Retained summary.<label>Read more</label></td></tr></table>')
            component = {'hash': 'one', 'label': 'Volume 1', 'record': {'description': ''}, 'source_entries': [{'source': 'gutenberg.org', 'entry_url': 'https://www.gutenberg.org/ebooks/1', 'metadata': {}}]}
            description, evidence = TOOL.recovery({'components': [component]}, {'https://www.gutenberg.org/ebooks/1': path})
            self.assertEqual(description, 'Retained summary.')
            self.assertEqual(evidence[0]['kind'], 'retained_gutenberg_page')
            self.assertEqual(component['record']['description'], '')
