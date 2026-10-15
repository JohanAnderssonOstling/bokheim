import importlib.util
import json
import io
import contextlib
from pathlib import Path
import sqlite3
import tempfile
import unittest
import urllib.error
import urllib.parse
import zipfile
from unittest.mock import patch

from PIL import Image, ImageDraw


SPEC = importlib.util.spec_from_file_location(
    'gutenberg_covers', Path(__file__).parents[1] / 'tools/audit-gutenberg-covers.py')
covers = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(covers)


class CoverAuditTests(unittest.TestCase):
    def test_epub_scan_does_not_hold_database_read_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with sqlite3.connect(root / 'curation.sqlite3') as db:
                db.executescript('''
                    CREATE TABLE book(hash TEXT, title TEXT, author TEXT, record TEXT);
                    CREATE TABLE book_source(hash TEXT, source TEXT, entry_id TEXT, entry_url TEXT);
                    CREATE TABLE source_entry(source TEXT, entry_id TEXT, metadata TEXT);
                    INSERT INTO book VALUES('one','One','Author','{}'),('two','Two','Author','{}');
                    INSERT INTO book_source VALUES('one','gutenberg.org','1','url'),('two','gutenberg.org','2','url');
                ''')

            def scan_while_writer_updates(_):
                with sqlite3.connect(root / 'curation.sqlite3', timeout=0) as writer:
                    writer.execute("UPDATE book SET author='Updated'")
                return None, None

            with patch.object(covers, 'embedded_cover', side_effect=scan_while_writer_updates), \
                    contextlib.redirect_stdout(io.StringIO()):
                covers.audit(root, root / 'audit')
            rows = json.loads((root / 'audit/audit.json').read_text())
            self.assertEqual(len(rows), 2)
            self.assertTrue(all(row['classification'] == 'original_or_unknown' for row in rows))

    def test_displayed_cover_preserves_replacement_when_evidence_is_missing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'books').mkdir()
            (root / 'covers').mkdir()
            generated = Image.new('RGB', (1600, 2400), 'white')
            draw = ImageDraw.Draw(generated)
            draw.rectangle((0, 800, 1600, 2400), fill='#d900b5')
            draw.rectangle((500, 1200, 1600, 2000), fill='#122aff')
            data = io.BytesIO()
            generated.save(data, format='PNG')
            with zipfile.ZipFile(root / 'books/book.epub', 'w') as archive:
                archive.writestr('123-cover.png', data.getvalue())
            card = root / 'covers/book-card.jpg'
            self.assertTrue(covers.displayed_placeholder(root, 'book', '123-cover.png'))
            generated.resize((300, 450)).save(card, quality=85)
            self.assertTrue(covers.displayed_placeholder(root, 'book', '123-cover.png'))
            Image.new('RGB', (300, 450), '#173024').save(card)
            self.assertFalse(covers.displayed_placeholder(root, 'book', '123-cover.png'))

    def test_google_retries_service_failures_then_accepts_success(self):
        unavailable = urllib.error.HTTPError('https://google.example/search', 503, 'backendFailed', {}, None)
        with patch.object(covers, 'request', side_effect=[unavailable, unavailable, b'{}']) as request, \
             patch.object(covers.time, 'sleep') as sleep:
            self.assertEqual(covers.google_request('https://google.example/search'), b'{}')
        self.assertEqual(request.call_count, 3)
        self.assertEqual([call.args[0] for call in sleep.call_args_list], [1, 2])

    def test_google_retry_is_bounded_and_does_not_repeat_quota_failures(self):
        for status, expected_attempts in ((503, 4), (429, 1), (403, 1)):
            error = urllib.error.HTTPError('https://google.example/search', status, 'error', {}, None)
            with self.subTest(status=status), patch.object(covers, 'request', side_effect=error) as request, \
                 patch.object(covers.time, 'sleep') as sleep:
                with self.assertRaises(urllib.error.HTTPError):
                    covers.google_request('https://google.example/search')
                self.assertEqual(request.call_count, expected_attempts)
                self.assertEqual(sleep.call_count, expected_attempts - 1)

    def test_generated_cover_requires_matching_book_id_name_and_dimensions(self):
        for name in ('OEBPS/1656-cover.png', 'OEBPS/123456_1656-cover.png'):
            self.assertTrue(covers.is_placeholder(name, [1600, 2400], '1656'))
        for name in ('OEBPS/cover.png', 'OEBPS/123_cover.jpg',
                     'OEBPS/1657-cover.png', 'OEBPS/art_1656-cover.png'):
            self.assertFalse(covers.is_placeholder(name, [1600, 2400], '1656'))
        self.assertFalse(covers.is_placeholder('1656-cover.png', [800, 1200], '1656'))
        self.assertFalse(covers.is_placeholder(None, None, '1656'))

    def test_openlibrary_lookup_rejects_other_authors_and_collections(self):
        row = dict(title='Utilitarianism', record=dict(contributors=[
            dict(name='John Stuart Mill', role=[97, 117, 116]),
            dict(name='Other Writer', role=[116, 114, 108])]))
        docs = [dict(title='Utilitarianism and Other Essays', author_name=['John Stuart Mill'], cover_i=1),
                dict(title='Utilitarianism', author_name=['Other Writer'], cover_i=2),
                dict(title='Utilitarianism', author_name=['John Stuart Mill']),
                dict(title='Utilitarianism', author_name=['John Stuart Mill'], cover_i=3)]
        with patch.object(covers, 'request', return_value=json.dumps(dict(docs=docs)).encode()) as request:
            match, evidence = covers.openlibrary_cover(row)
        self.assertEqual(match['cover_i'], 3)
        self.assertEqual(evidence['response']['docs'], docs)
        self.assertIn('author=John+Stuart+Mill', request.call_args.args[0])

    def test_openlibrary_lookup_without_author_does_not_guess(self):
        with patch.object(covers, 'request') as request:
            self.assertEqual(covers.openlibrary_cover(dict(title='Apology', record=dict(contributors=[]))), (None, None))
        request.assert_not_called()

    def test_openlibrary_strict_matches_precede_looser_matches(self):
        row = dict(title='The Republic', record=dict(contributors=[
            dict(name='Plato', role=[97, 117, 116])], book=dict(languages=['en'])))
        docs = [dict(title='Republic: An Introduction', author_name=['Πλάτων'], cover_i=1, language=['eng']),
                dict(title='The Republic', author_name=['Plato'], cover_i=2, language=['eng']),
                dict(title='The Republic', author_name=['Plato'], cover_i=3, language=['fre']),
                dict(title='Republic: Selected Extracts', author_name=['Plato'], cover_i=4),
                dict(title='The Republic and Other Dialogues', author_name=['Plato'], cover_i=5),
                dict(title='Republic', author_name=['Another Author'], cover_i=6),
                dict(title='The Republic', author_name=['Plato'], cover_i=2)]
        matches = covers.openlibrary_candidates(row, dict(docs=docs))
        self.assertEqual([(doc['cover_i'], tier) for doc, tier in matches], [(2, 'strict'), (1, 'loose')])

    def test_looser_author_matching_accepts_initials_but_rejects_conflicts(self):
        self.assertTrue(covers.author_matches('John Stuart Mill', 'J. S. Mill'))
        self.assertTrue(covers.author_matches('John Stuart Mill', 'Mill, John Stuart, 1806-1873'))
        self.assertTrue(covers.author_matches('Arthur Conan Doyle', 'Arthur C. Doyle'))
        self.assertFalse(covers.author_matches('John Stuart Mill', 'James Mill'))
        self.assertFalse(covers.author_matches('John Stuart Mill', 'J. R. Mill'))
        self.assertFalse(covers.author_matches('John Stuart Mill', 'Mill'))

    def test_author_matching_removes_baron_and_adjacent_duplicates(self):
        source = 'Baron Thomas Babington Macaulay Macaulay'
        self.assertTrue(covers.author_matches(source, 'Thomas Babington Macaulay', strict=True))
        self.assertTrue(covers.author_matches(source, 'Macaulay Macaulay, Baron Thomas Babington'))
        self.assertTrue(covers.author_matches(source, 'Thomas B. Macaulay'))
        self.assertFalse(covers.author_matches(source, 'Thomas James Macaulay'))
        self.assertEqual(covers.author_name_match_key('Thomas Thomas Macaulay'), 'thomas macaulay')
        self.assertEqual(covers.author_name_match_key('John Smith John'), 'john smith john')
        self.assertEqual(covers.author_name_match_key('John Baron Smith'), 'john baron smith')
        self.assertFalse(covers.author_matches('John Smith John', 'John Smith'))

    def test_cover_providers_accept_normalized_macaulay_credit(self):
        row = dict(title='Lays of Ancient Rome', record=dict(
            title='Lays of Ancient Rome', contributors=[dict(
                name='Baron Thomas Babington Macaulay Macaulay', role=[97, 117, 116])],
            book=dict(languages=['en'])))
        docs = [dict(title=row['title'], author_name=['Thomas Babington Macaulay'], cover_i=1)]
        self.assertEqual([(doc['cover_i'], tier) for doc, tier in
                          covers.openlibrary_candidates(row, dict(docs=docs))], [(1, 'strict')])
        self.assertTrue(covers.google_matches(row, dict(title=row['title'],
                       authors=['Thomas Babington Macaulay'], language='en')))

    def test_reordered_author_words_are_only_loose_matches(self):
        for local, remote in [('da Vinci Leonardo', 'Leonardo da Vinci'),
                              ('the Younger Pliny', 'Pliny the Younger'),
                              ('Andrew W. Young', 'W. Andrew Young')]:
            with self.subTest(local=local):
                self.assertFalse(covers.author_matches(local, remote, strict=True))
                self.assertTrue(covers.author_matches(local, remote))
                self.assertTrue(covers.author_matches(remote, local))
        self.assertTrue(covers.author_matches('John S. Smith', 'J. Stuart Smith'))
        for local, remote in [('John J. Smith', 'James J. Smith'),
                              ('John M. Smith', 'John S. Smith'),
                              ('John Smith John', 'John Smith'),
                              ('Bernard Shaw', 'George Bernard Shaw'),
                              ('John Smith Jr', 'John Smith Sr')]:
            with self.subTest(local=local):
                self.assertFalse(covers.author_matches(local, remote))
                self.assertFalse(covers.author_matches(remote, local))

    def test_dates_do_not_drop_the_surname_or_other_name_parts(self):
        for name in ['John M. 1851-1929 Tyler', 'John M. (1851–1929) Tyler',
                     'Tyler, John M., 1851—1929', 'John M. b. 1851 Tyler',
                     'John M. died 1929 Tyler', 'John M. 1851- Tyler']:
            with self.subTest(name=name):
                self.assertTrue(covers.author_matches(name, 'John M. Tyler'))
        self.assertTrue(covers.author_matches('Lillian Moller 1878-1972 Gilbreth', 'Lillian Moller Gilbreth', strict=True))
        self.assertTrue(covers.author_matches('Reginald R. b. 1848 Sharpe', 'Reginald R. Sharpe', strict=True))
        self.assertEqual(covers.author_name_match_key('John D. Tyler'), 'john d tyler')
        self.assertTrue(covers.author_matches('John B. 1851-1929 Tyler', 'John B. Tyler', strict=True))
        self.assertTrue(covers.author_matches('John D. 1851-1929 Tyler', 'John D. Tyler', strict=True))
        self.assertEqual(covers.author_name_match_key('John 12345 Tyler'), 'john 12345 tyler')

    def test_openlibrary_strict_author_match_wins_over_earlier_reordered_name(self):
        row = dict(title='The Notebooks', record=dict(contributors=[
            dict(name='da Vinci Leonardo', role=[97, 117, 116])]))
        docs = [dict(title=row['title'], author_name=['Leonardo da Vinci'], cover_i=1),
                dict(title=row['title'], author_name=['da Vinci Leonardo'], cover_i=2)]
        matches = covers.openlibrary_candidates(row, dict(docs=docs))
        self.assertEqual([(doc['cover_i'], tier) for doc, tier in matches], [(2, 'strict'), (1, 'loose')])

    def test_google_strict_author_match_wins_over_earlier_reordered_name(self):
        row = dict(title='The Notebooks', record=dict(title='The Notebooks',
                   contributors=[dict(name='da Vinci Leonardo', role=[97, 117, 116])], book=dict(languages=['en'])))
        def volume(key, author):
            return dict(id=key, volumeInfo=dict(title=row['title'], authors=[author], language='en',
                        imageLinks=dict(thumbnail='https://books.google.com/books/content?id='+key)))
        with patch.object(covers, 'request', return_value=json.dumps(dict(items=[
                volume('loose', 'Leonardo da Vinci'), volume('strict', 'da Vinci Leonardo')])).encode()):
            candidates, _ = covers.google_cover(row, 'private-test-key')
        self.assertEqual([(c['volume_id'], c['match_tier']) for c in candidates], [('strict', 'strict'), ('loose', 'loose')])

    def test_looser_titles_preserve_volume_identity(self):
        row = dict(title='History of England, Volume 2')
        self.assertTrue(covers.loose_title_matches(row, 'The History of England, Vol. II'))
        self.assertFalse(covers.loose_title_matches(row, 'History of England, Volume 1'))
        self.assertFalse(covers.loose_title_matches(row, 'History of England'))
        self.assertTrue(covers.loose_title_matches(row, 'History of England (Vol. II of 3)'))
        self.assertFalse(covers.loose_title_matches(row, 'History of England (Vol. I of 3)'))

    def test_subtitle_formatting_uses_known_title_and_subtitle_text(self):
        cases = [
            ('The Foundations of the Origin of Species / Two Essays written in 1842 and 1844', None,
             'Foundations of the Origin of Species Two Essays Written in 1842 And 1844'),
            ('The Government Class Book / Designed for the Instruction of Youth', None,
             'The Government Class Book (Designed for the Instruction of Youth)'),
            ('Renaissance in Italy, Volume 1 / The Age of the Despots', None,
             'Renaissance in Italy, Vol. I (of 7) The Age of the Despots'),
            ('Critical Miscellanies, Vol. 1, Essay 5', 'Emerson',
             'Critical Miscellanies, Vol. 1, Essay 5, Emerson'),
        ]
        for title, subtitle, found in cases:
            row = dict(title=title, record=dict(subtitle=subtitle))
            with self.subTest(title=title):
                self.assertTrue(covers.loose_title_matches(row, found))
        row = dict(title=cases[0][0], record=dict(contributors=[dict(name='Charles Darwin', role=[97, 117, 116])]))
        docs = [dict(title=cases[0][2], author_name=['Charles Darwin'], cover_i=1),
                dict(title=row['title'], author_name=['Charles Darwin'], cover_i=2)]
        self.assertEqual([(doc['cover_i'], tier) for doc, tier in
                          covers.openlibrary_candidates(row, dict(docs=docs))], [(2, 'strict'), (1, 'loose')])

    def test_title_formatting_preserves_different_contents(self):
        self.assertFalse(covers.loose_title_matches(dict(title='History of the Wars, Books I and II'),
                                                   'History of the Wars, Books III and IV'))
        self.assertTrue(covers.loose_title_matches(dict(title='History of the Wars, Books I and II'),
                                                  'History of the Wars, Books I-II'))
        self.assertFalse(covers.loose_title_matches(dict(title='Critical Miscellanies, Vol. 1, Essay 5'),
                                                   'Critical Miscellanies, Vol. 1, Essay 2'))
        self.assertFalse(covers.loose_title_matches(dict(title='Utilitarianism'), 'Utilitarianism: Other Essays'))
        self.assertFalse(covers.loose_title_matches(dict(title='Utilitarianism'), 'Utilitarianism (Abridged)'))
        self.assertFalse(covers.loose_title_matches(dict(title='Étude Médico-Légale', record=dict(subtitle='Psychopathia Sexualis')),
                                                   'Étude Médico-Légale sur la simulation de la folie'))
        self.assertFalse(covers.loose_title_matches(dict(title='Essays', record=dict(subtitle='On Women')),
                                                   'Essays: On Religion'))

    def test_openlibrary_probe_exhausts_strict_covers_before_loose_fallback(self):
        row = dict(hash='book', entry_id='123', title='The Republic', classification='generated',
                   displayed_placeholder=True, record=dict(contributors=[
                       dict(name='Plato', role=[97, 117, 116])], book=dict(languages=['en'])))
        docs = [dict(title='Republic: A Study', author_name=['Plato'], cover_i=3),
                dict(title='The Republic', author_name=['Plato'], cover_i=1),
                dict(title='The Republic', author_name=['Plato'], cover_i=2)]
        image = io.BytesIO()
        Image.new('RGB', (300, 450), 'blue').save(image, format='JPEG')
        urls = []

        def fetch(url):
            urls.append(url)
            if 'search.json' in url:
                return json.dumps(dict(docs=docs)).encode()
            if '/1-L.jpg' in url or '/2-L.jpg' in url:
                raise urllib.error.HTTPError(url, 404, 'missing', {}, None)
            return image.getvalue()

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            covers.save_json(root / 'audit.json', [row])
            with patch.object(covers, 'request', side_effect=fetch), \
                    patch.object(covers.time, 'sleep'), patch.object(covers, 'comparison'), \
                    contextlib.redirect_stdout(io.StringIO()):
                covers.probe(root, root, 'https://unused.example/', 1, openlibrary=True)
            result = json.loads((root / 'probe.json').read_text())[0]
            self.assertEqual(result['match_tier'], 'loose')
            self.assertEqual(result['matched_work']['cover_i'], 3)
        self.assertEqual([url.rsplit('/', 1)[-1] for url in urls[1:]],
                         ['1-L.jpg?default=false', '2-L.jpg?default=false', '3-L.jpg?default=false'])

    def test_google_lookup_verifies_work_and_preserves_frontcover_parameters(self):
        row = dict(title='Utilitarianism', record=dict(title='Utilitarianism', subtitle=None,
                   contributors=[dict(name='John Stuart Mill', role=[97, 117, 116])], book=dict(languages=['en'])))
        right = dict(title='Utilitarianism', authors=['John Stuart Mill'], language='en',
                     imageLinks=dict(thumbnail='http://books.google.com/books/content?id=right&img=1&zoom=1'))
        wrong_author = dict(right, authors=['Other Author'])
        collection = dict(right, subtitle='On Liberty and Other Essays')
        wrong_language = dict(right, language='fr')
        unsafe = dict(right, imageLinks=dict(thumbnail='https://other.example/cover.jpg'))
        docs = [dict(id=str(index), volumeInfo=info) for index, info in enumerate(
            [wrong_author, collection, wrong_language, unsafe, right])]
        with patch.object(covers, 'request', return_value=json.dumps(dict(items=docs)).encode()) as request:
            matches, receipt = covers.google_cover(row, 'private-test-key')
        query = urllib.parse.parse_qs(urllib.parse.urlsplit(request.call_args.args[0]).query)
        self.assertEqual(query['q'], ['Utilitarianism John Stuart Mill'])
        self.assertEqual(len(matches), 1)
        self.assertEqual(matches[0]['volume_id'], '4')
        self.assertIn('zoom=1', matches[0]['image_url'])
        self.assertIn('w=1280', matches[0]['image_url'])
        self.assertTrue(matches[0]['image_url'].startswith('https://books.google.com/'))
        self.assertNotIn('private-test-key', json.dumps(receipt))

    def test_edition_discovery_finds_cover_missing_from_work_search(self):
        row = dict(title='Public opinion', record=dict(contributors=[
            dict(name='Walter Lippmann', role=[97, 117, 116])], book=dict(languages=['en'])))
        work = dict(key='/works/OL42346W', title='Public opinion', author_name=['Walter Lippmann'],
                    author_key=['OL30891A'], cover_i=5847876, language=['eng'])
        edition = dict(key='/books/OL7639441M', title='Public Opinion', covers=[314247, -1, 314247],
                       publishers=['Dover Publications'], languages=[dict(key='/languages/eng')],
                       authors=[dict(key='/authors/OL30891A')], works=[dict(key='/works/OL42346W')])
        evidence = []
        with patch.object(covers, 'request', return_value=json.dumps(dict(size=1, entries=[edition])).encode()) as request:
            candidates = list(covers.openlibrary_cover_candidates(row, dict(docs=[work]), evidence))
        self.assertEqual([doc['cover_i'] for doc, _ in candidates], [314247, 5847876])
        self.assertEqual(candidates[0][0]['cover_edition_key'], '/books/OL7639441M')
        self.assertIn('/works/OL42346W/editions.json?', request.call_args.args[0])
        self.assertEqual(len(evidence), 1)

    def test_edition_discovery_paginates_and_checks_language_volume_and_author(self):
        row = dict(title='History of England', record=dict(subtitle='Volume 2', contributors=[
            dict(name='Author Name', role=[97, 117, 116])], book=dict(languages=['en'])))
        work = dict(key='/works/OL1W', title='History of England', author_name=['Author Name'], author_key=['OL1A'])
        def edition(key, **overrides):
            return dict(dict(key=key, title='History of England, Volume 2', covers=[10],
                             works=[dict(key='/works/OL1W')], authors=[dict(key='/authors/OL1A')],
                             languages=[dict(key='/languages/eng')]), **overrides)
        first = [edition('/books/OL1M', languages=[dict(key='/languages/fre')]),
                 edition('/books/OL2M', title='History of England, Volume 1'),
                 edition('/books/OL3M', authors=[dict(key='/authors/OL2A')]),
                 edition('/books/OL4M', works=[dict(key='/works/OL2W')])]
        # A full first page is followed by another page, rather than relying on
        # remote next URLs, which may not be trustworthy.
        first += [edition('/books/OL'+str(100+n)+'M', covers=[]) for n in range(96)]
        pages=[dict(size=101, entries=first), dict(size=101, entries=[edition('/books/OL5M')])]
        with patch.object(covers, 'request', side_effect=[json.dumps(p).encode() for p in pages]) as request:
            candidates=covers.openlibrary_edition_candidates(row,work,[])
        self.assertEqual(request.call_count, 2)
        self.assertIn('offset=100', request.call_args.args[0])
        self.assertEqual([(doc['cover_edition_key'],doc['cover_i']) for doc,_ in candidates],
                         [('/books/OL5M',10)])

    def test_probe_stages_multiple_valid_covers_for_review_but_one_manifest_entry(self):
        row=dict(hash='book',entry_id='123',title='Public Opinion',classification='generated',
                 displayed_placeholder=True,record=dict(contributors=[dict(name='Walter Lippmann',role=[97,117,116])]))
        docs=[dict(title='Public Opinion',author_name=['Walter Lippmann'],cover_i=i) for i in (1,2)]
        data=io.BytesIO();Image.new('RGB',(300,450),'purple').save(data,format='JPEG')
        def fetch(url):return json.dumps(dict(docs=docs)).encode() if 'search.json' in url else data.getvalue()
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);covers.save_json(root/'audit.json',[row])
            with patch.object(covers,'request',side_effect=fetch),patch.object(covers.time,'sleep'), \
                    patch.object(covers,'comparison'),contextlib.redirect_stdout(io.StringIO()):
                covers.probe(root,root,'https://unused.example/',1,openlibrary=True)
            answer=json.loads((root/'probe.json').read_text())[0]
            self.assertEqual([c['matched_work']['cover_i'] for c in answer['cover_candidates']],[1,2])
            self.assertTrue(all((root/c['file_path']).exists() for c in answer['cover_candidates']))
            self.assertEqual(len(json.loads((root/'retain-covers.json').read_text())),1)

    def test_coverless_work_can_supply_edition_cover_and_strict_stays_first(self):
        row=dict(title='The Republic',record=dict(contributors=[dict(name='Plato',role=[97,117,116])],book=dict(languages=['en'])))
        strict=dict(key='/works/OL1W',title='The Republic',author_name=['Plato'])
        loose=dict(key='/works/OL2W',title='Republic: A Study',author_name=['Plato'],cover_i=2)
        edition=dict(key='/books/OL1M',title='The Republic',covers=[1],languages=[dict(key='/languages/eng')])
        with patch.object(covers,'request',side_effect=[json.dumps(dict(size=1,entries=[edition])).encode(),b'{"size":0,"entries":[]}']):
            candidates=list(covers.openlibrary_cover_candidates(row,dict(docs=[loose,strict]),[]))
        self.assertEqual([(doc['cover_i'],tier) for doc,tier in candidates],[(1,'strict'),(2,'loose')])

    def test_edition_discovery_is_bounded_and_ignores_untrusted_keys(self):
        row=dict(title='Book',record=dict(contributors=[dict(name='Author',role=[97,117,116])]))
        work=dict(key='/works/OL1W',title='Book',author_name=['Author'])
        page=dict(size=10000,entries=[dict(key='/books/OL'+str(n)+'M',title='Book',covers=[]) for n in range(100)])
        with patch.object(covers,'request',return_value=json.dumps(page).encode()) as request:
            self.assertEqual(covers.openlibrary_edition_candidates(row,work,[],max_pages=2),[])
        self.assertEqual(request.call_count,2)
        with patch.object(covers,'request') as request:
            self.assertEqual(covers.openlibrary_edition_candidates(row,dict(work,key='https://other.example/works/OL1W'),[]),[])
        request.assert_not_called()


if __name__ == '__main__':
    unittest.main()
