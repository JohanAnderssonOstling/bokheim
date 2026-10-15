import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
import zipfile


SPEC = importlib.util.spec_from_file_location('unglue', Path(__file__).parents[1] / 'tools/ingest-unglue.py')
unglue = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(unglue)

PAGE = '''<h2 itemprop="name">Programming Examples</h2>
<span itemprop="author">Example Author</span>
<span itemprop="datePublished">2004</span><span itemprop="inLanguage">en</span>
<div>4 Ungluers have <b>Faved this Work</b></div>
<div itemprop="description"><p>Learn programming.</p></div>
<li itemprop="keywords">Computers (244)</li>
<div>This work has been downloaded 1,386 times via unglue.it ebook links.
<ol><li>1,000 - epub (CC BY) at Unglue.it.</li><li>386 - pdf (GFDL) at Unglue.it.</li></ol></div>
<a class="find-openlibrary" href="/work/42/openlibrary/">Open Library</a>
<a class="find-librarything" href="/work/42/librarything/">LibraryThing</a>
<div id="edition_1">Contributors:\n Example Author, <br>
1 ebooks<br>Publisher: Example Press<br>Published: 2004<br>
ISBN: <span itemprop="isbn">9780975283844</span><br>
DOI: <a href="https://doi.org/10.1234/example">10.1234/example</a><br>
OCLC: 55959463<br></div>
<div id="edition_2">Published: 2026<br>ISBN: <span itemprop="isbn">9781540831828</span><br></div>
'''


def row(**changes):
    value = dict(entry_id='42', title='Programming Examples', year=2004, faves=4,
                 subjects=['Computers'], categories=['https://unglue.it/free/kw.Computers/'],
                 acquisitions=[dict(url='https://unglue.it/download_ebook/1/', format='epub', rights='CC BY')])
    value.update(changes)
    return value


def feed(entries, next_link=''):
    return f'''<feed xmlns="http://www.w3.org/2005/Atom" xmlns:d="http://purl.org/dc/terms/">
    {next_link}{entries}</feed>'''.encode()


ENTRY = '''<entry><id>https://unglue.it/api/id/work/42/</id><title>Feed title</title>
<d:issued>2026</d:issued><d:language>en</d:language>
<link rel="http://opds-spec.org/acquisition/open-access" type="application/epub+zip" href="/download_ebook/1/" d:rights="CC BY"/>
<link rel="http://opds-spec.org/acquisition/buy" type="application/pdf" href="https://shop.example/buy"/>
<category term="Computers"/></entry>'''


class UnglueTests(unittest.TestCase):
    def test_download_page_finds_epub_and_pdf_missing_from_feed(self):
        page = '''<div class="ebook_download">
        <a href="/download_ebook/1/"><img alt="CC BY-NC"/></a>
        <a href="/download_ebook/1/">epub at Internet Archive</a>
        <a href="/download_ebook/2/"><img alt="pdf at Unglue.it"/></a>
        <a href="/download_ebook/2/" data-filename="book.pdf"></a>
        <a href="/download_ebook/3/">mobi at Unglue.it</a></div>
        <a href="https://shop.example/buy">Buy PDF</a>'''
        links = unglue.download_page_acquisitions(page, 'https://unglue.it/work/42/download/')
        self.assertEqual([link['format'] for link in links], ['epub', 'pdf', 'mobi'])
        self.assertEqual(links[0]['rights'], 'CC BY-NC')
        self.assertEqual(links[0]['url'], 'https://unglue.it/download_ebook/1/')

    def test_website_metadata_preserves_counts_identifiers_editions_and_provider_links(self):
        metadata = unglue.page_metadata(PAGE, 'https://unglue.it/work/42/')
        self.assertEqual((metadata['faves'], metadata['downloads']), (4, 1386))
        self.assertEqual([r['count'] for r in metadata['download_breakdown']], [1000, 386])
        self.assertEqual(metadata['year'], 2004)
        self.assertEqual(metadata['subjects'], ['Computers'])
        self.assertEqual(metadata['editions'][0]['fields']['contributors'], ['Example Author,'])
        self.assertEqual(metadata['provider_links']['openlibrary'][0]['url'], 'https://unglue.it/work/42/openlibrary/')
        self.assertEqual(metadata['provider_links']['librarything'][0]['url'], 'https://unglue.it/work/42/librarything/')
        identifiers = metadata['identifiers']
        self.assertEqual(len([item for item in identifiers if item['scheme'] == 'doi']), 1)
        self.assertTrue(next(item for item in identifiers if item['scheme'] == 'doi')['downloadable_edition'])
        self.assertFalse(next(item for item in identifiers if item['value'] == '9781540831828')['downloadable_edition'])

    def test_faves_are_never_inferred_from_thanks_downloads_or_feed_rating(self):
        self.assertIsNone(unglue.fave_count('Thanked by 5; downloaded 100 times; ratingValue="3"'))
        self.assertEqual(unglue.fave_count('1 Ungluer has Faved this Work'), 1)
        self.assertEqual(unglue.fave_count('0 Ungluers have Faved this Work'), 0)
        self.assertIsNone(unglue.fave_count('2 Ungluers have Faved this Work; 3 Ungluers have Faved this Work'))
        self.assertEqual(unglue.fave_count('<div>3 ungluers thanked</div><li id="supporters"><a>Ungluers<br>(36)</a></li>'), 36)
        self.assertEqual(unglue.fave_count('<li id="supporters"><a>Ungluers</a></li>'), 0)

    def test_review_gate_and_obsolete_skip(self):
        self.assertEqual(unglue.classify(row(title='Ubuntu 14.04 Manual'), 2026, 5)[0], 'skip')
        self.assertEqual(unglue.classify(row(title='Ubuntu Server Guide 14.04 LTS'), 2026, 5)[0], 'skip')
        self.assertEqual(unglue.classify(row(faves=0), 2026, 5)[0], 'skip')
        self.assertEqual(unglue.classify(row(faves=None), 2026, 5)[0], 'review')
        self.assertEqual(unglue.classify(row(), 2026, 5)[0], 'review')
        self.assertEqual(unglue.classify(row(year=None), 2026, 5)[0], 'review')
        self.assertEqual(unglue.classify(row(year=2021), 2026, 5)[0], 'review')
        self.assertEqual(unglue.classify(row(year=2022), 2026, 5)[0], 'include')
        self.assertEqual(unglue.classify(row(title='Rails 4.0 Manual', year=2026), 2026, 5)[0], 'review')
        self.assertEqual(unglue.classify(row(title='Classic novel', subjects=['Fiction'], categories=[], year=1850), 2026, 5)[0], 'include')

    def test_review_cannot_override_faves_or_skipped_obsolete_books(self):
        selection = dict(complete=True, books=[row(decision='review'), row(entry_id='43', decision='review', faves=None),
                                              row(entry_id='44', decision='skip')])
        self.assertEqual(list(unglue.approved_books(selection, [])), [])
        self.assertEqual([r['entry_id'] for r in unglue.approved_books(selection,
                         [dict(entry_id='42', decision='include'), dict(entry_id='43', decision='include')])], ['42'])
        with self.assertRaises(ValueError):
            list(unglue.approved_books(selection, [dict(entry_id='44', decision='include')]))
        with self.assertRaises(ValueError):
            list(unglue.approved_books(dict(selection, complete=False), []))

    def test_category_mapping_preserves_filters_and_pagination(self):
        self.assertEqual(unglue.category_feed('https://unglue.it/free/kw.Computers/epub/?order_by=popular'),
                         'https://unglue.it/api/opds/kw.Computers/epub/?order_by=popular')
        self.assertEqual(unglue.category_feed('https://unglue.it/free/doab/'),
                         'https://unglue.it/api/opds/doab/-gtbg/')
        books, next_url = unglue.parse_feed(feed(ENTRY, '<link rel="next" href="?page=1"/>'),
                                           'https://unglue.it/api/opds/kw.Computers/')
        self.assertEqual(next_url, 'https://unglue.it/api/opds/kw.Computers/?page=1')
        self.assertEqual(books[0]['acquisitions'], [dict(url='https://unglue.it/download_ebook/1/', format='epub', rights='CC BY')])
        with self.assertRaises(ValueError):
            unglue.category_feed('https://other.example/free/books/')

    def test_gutenberg_provenance_blocks_manual_approval_and_redirects(self):
        page = PAGE + '<p>This book is included in <a href="https://www.gutenberg.org/ebooks/123">Project Gutenberg</a>.</p>'
        metadata = unglue.page_metadata(page, 'https://unglue.it/work/42/')
        self.assertTrue(metadata['gutenberg_origin'])
        book = row(decision='review', website_metadata=metadata)
        self.assertEqual(unglue.classify(book, 2026, 5)[0], 'skip')
        self.assertEqual(list(unglue.approved_books(dict(complete=True, books=[book]),
            [dict(entry_id='42', decision='include')])), [])
        with self.assertRaises(unglue.ExcludedOrigin):
            unglue.acquisition_url('https://www.gutenberg.org/files/123/book.epub', None, None)
        with self.assertRaises(unglue.ExcludedOrigin):
            unglue.acquisition_url('https://github.com/GITenberg/book/ebook.epub', None, None)
        self.assertFalse(unglue.gutenberg_url('https://gutenberg.org.example/book.epub'))
        self.assertFalse(unglue.page_metadata(PAGE.replace('Learn programming.', 'An essay mentioning Project Gutenberg.'),
            'https://unglue.it/work/42/')['gutenberg_origin'])
        self.assertFalse(unglue.page_metadata(PAGE.replace('Learn programming.',
            'An essay citing <a href="https://www.gutenberg.org/ebooks/123">a Gutenberg book</a>.'),
            'https://unglue.it/work/42/')['gutenberg_origin'])

    def test_feed_gutenberg_identifier_and_download_provider_are_provenance(self):
        entry = ENTRY.replace('</entry>', '<d:identifier>urn:gtbg:123</d:identifier></entry>')
        books, _ = unglue.parse_feed(feed(entry), 'https://unglue.it/api/opds/')
        self.assertTrue(books[0]['gutenberg_origin'])
        book = row(website_metadata=dict(download_breakdown=[dict(details='(PD-US) at Project Gutenberg')]))
        self.assertTrue(unglue.has_gutenberg_origin(book))

    def test_epub_publisher_excludes_gutenberg_from_other_mirrors(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'book.epub'
            for publisher, expected in [('Project Gutenberg', True), ('Other Publisher', False)]:
                with zipfile.ZipFile(path, 'w') as archive:
                    archive.writestr('META-INF/container.xml', '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>')
                    archive.writestr('content.opf', f'<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:publisher>{publisher}</dc:publisher><dc:description>A reference to Project Gutenberg</dc:description></metadata></package>')
                self.assertEqual(unglue.epub_gutenberg_origin(path), expected)

    def test_prepare_uses_website_year_and_keeps_manual_decisions_on_resume(self):
        class FakeFetcher:
            def fetch(self, url, path):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(PAGE.encode() if '/work/' in url else feed(ENTRY))
                return dict(observed_at='2026-10-12T12:00:00Z')
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(output=Path(directory), categories=['https://unglue.it/free/kw.Computers/'], review_age=5)
            unglue.prepare(args, FakeFetcher())
            selection = unglue.read_json(args.output / 'selection.json')
            self.assertEqual(selection['books'][0]['year'], 2004)
            self.assertEqual(selection['books'][0]['feed_metadata']['year'], 2026)
            review = unglue.read_json(args.output / 'review.json')
            review[0]['decision'] = 'include'
            unglue.write_json(args.output / 'review.json', review)
            unglue.prepare(args, FakeFetcher())
            self.assertEqual(unglue.read_json(args.output / 'review.json')[0]['decision'], 'include')
            self.assertFalse((args.output / 'books').exists())

    def test_download_retains_evidence_and_holds_unreviewed_books(self):
        class FakeFetcher:
            calls = []
            def fetch(self, url, path, resolver):
                self.calls.append(url)
                path.parent.mkdir(parents=True, exist_ok=True)
                with zipfile.ZipFile(path, 'w') as archive:
                    archive.writestr('mimetype', 'application/epub+zip')
                    archive.writestr('META-INF/container.xml', '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>')
                    archive.writestr('content.opf', '<package xmlns="http://www.idpf.org/2007/opf"><metadata/></package>')
                return dict(url=url, response_url=url, observed_at='2026-10-12')
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            metadata = unglue.page_metadata(PAGE, 'https://unglue.it/work/42/')
            book = row(decision='include', website_metadata=metadata, description=metadata['description'],
                       languages=['en'], entry_url='https://unglue.it/work/42/', observed_at='2026-10-12')
            unglue.write_json(output / 'selection.json', dict(complete=True, books=[book, row(entry_id='43', decision='review')]))
            unglue.write_json(output / 'review.json', [])
            fetcher = FakeFetcher()
            unglue.download(SimpleNamespace(output=output, review=None), fetcher)
            self.assertEqual(len(fetcher.calls), 1)
            manifest = unglue.read_json(output / 'import-manifest.json')[0]
            self.assertEqual(manifest['source_evidence']['website_metadata']['downloads'], 1386)
            self.assertEqual({r['value'] for r in manifest['metadata']['identifiers']},
                             {'9780975283844', '9781540831828', '10.1234/example', '55959463'})
            self.assertEqual([r['scope'] for r in manifest['metadata']['identifiers'] if r['scheme'] == 'isbn'], ['book', 'book'])

    def test_isbn_checksums_and_nonbook_download_rejection(self):
        self.assertIsNone(unglue.valid_isbn('9780975283845'))
        self.assertEqual(unglue.valid_isbn('978-0-9752838-4-4'), '9780975283844')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'book.pdf'
            path.write_text('<html>Login to download</html>')
            with self.assertRaises(ValueError):
                unglue.validate_book(path, 'pdf')

    def test_pdf_download_creates_source_link_and_prefers_declared_license(self):
        class FakeFetcher:
            calls = []
            def fetch(self, url, path, resolver):
                self.calls.append(url)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'%PDF-1.4\n%%EOF')
                return dict(url=url, response_url=url)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            website = unglue.page_metadata(PAGE, 'https://unglue.it/work/42/')
            book = row(decision='include', website_metadata=website, description=website['description'],
                       languages=['en'], authors=['Example Author'], entry_url='https://unglue.it/work/42/',
                       observed_at='2026-10-12', acquisitions=[
                           dict(url='https://unglue.it/download_ebook/1/', format='pdf', rights='None'),
                           dict(url='https://unglue.it/download_ebook/2/', format='pdf', rights='CC BY')])
            unglue.write_json(output / 'selection.json', dict(complete=True, books=[book]))
            unglue.write_json(output / 'review.json', [])
            fetcher = FakeFetcher()
            unglue.download(SimpleNamespace(output=output, review=None), fetcher)
            self.assertEqual(fetcher.calls, ['https://unglue.it/download_ebook/2/'])
            self.assertEqual(unglue.read_json(output / 'import-manifest.json'), [])
            entry = unglue.read_json(output / 'link-manifest.json')[0]
            self.assertEqual(entry['format'], 'Pdf')
            self.assertEqual(entry['record']['title'], book['title'])
            self.assertEqual(len([i for i in entry['record']['book']['identifiers'] if i['scheme'] == 'isbn']), 2)
            self.assertEqual(entry['source_evidence']['acquisition']['status'], 'downloaded')

    def test_alternate_advertised_link_is_used_after_a_failure(self):
        class FakeFetcher:
            def fetch(self, url, path, resolver):
                if url.endswith('/1/'):
                    raise ValueError('Advertised mirror unavailable')
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'%PDF-1.4\n%%EOF')
                return dict(url=url, response_url=url)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            website = unglue.page_metadata(PAGE, 'https://unglue.it/work/42/')
            book = row(decision='include', website_metadata=website, description=website['description'],
                       languages=['cn'], authors=['Example Author'], entry_url='https://unglue.it/work/42/',
                       observed_at='2026-10-12', acquisitions=[
                           dict(url=f'https://unglue.it/download_ebook/{i}/', format='pdf', rights='CC BY') for i in (1, 2)])
            unglue.write_json(output / 'selection.json', dict(complete=True, books=[book]))
            unglue.write_json(output / 'review.json', [])
            unglue.download(SimpleNamespace(output=output, review=None), FakeFetcher())
            entry = unglue.read_json(output / 'link-manifest.json')[0]
            self.assertEqual(entry['file_url'], 'https://unglue.it/download_ebook/2/')
            self.assertEqual(entry['record']['book']['languages'], ['zh'])
            attempts = entry['source_evidence']['acquisition']['failed_options']
            self.assertEqual(attempts[0]['url'], 'https://unglue.it/download_ebook/1/')


if __name__ == '__main__':
    unittest.main()
