#!/usr/bin/env python3
"""Freeze a Gutenberg bookshelf's popularity selection, then import its EPUB3s.

prepare writes only acquisition evidence. run ingests that explicit selection
into the local catalog, retaining pages and progress so interrupted runs resume.
"""
import argparse
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import fcntl
import hashlib
from html.parser import HTMLParser
import json
import logging
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import threading
import tarfile
import time
import unicodedata
from urllib.parse import urljoin, urlparse
from urllib.error import HTTPError
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET
import zipfile

ORIGIN = 'https://www.gutenberg.org'
CATEGORIES = {658: ('History - European', 'history-european'), 659: ('History - Ancient', 'history-ancient'), 688: ('Psychiatry/Psychology', 'psychology'), 691: ('Philosophy & Ethics', 'philosophy-ethics'), 693: ('Sociology', 'sociology'), 694: ('Politics', 'politics')}


def now():
    return datetime.now(timezone.utc).isoformat(timespec='seconds')


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


class Listing(HTMLParser):
    def __init__(self):
        super().__init__()
        self.books = []
        self.book = None
        self.field = None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        classes = attrs.get('class', '').split()
        if tag == 'li' and 'booklink' in classes:
            self.book = {'title': '', 'author': '', 'extra': ''}
        if self.book is not None and tag == 'a':
            match = re.fullmatch(r'/ebooks/(\d+)', attrs.get('href', ''))
            if match:
                self.book['entry_id'] = match[1]
        if self.book is not None and tag == 'span':
            self.field = next((field for name, field in [('title', 'title'), ('subtitle', 'author'), ('extra', 'extra')] if name in classes), None)

    def handle_data(self, data):
        if self.book is not None and self.field:
            self.book[self.field] += data

    def handle_endtag(self, tag):
        if tag == 'span':
            self.field = None
        if tag == 'li' and self.book is not None:
            if 'entry_id' in self.book:
                match = re.search(r'([\d,]+)\s+downloads', self.book.pop('extra'))
                if not match:
                    raise ValueError('Listing has no popularity count for ' + self.book['entry_id'])
                self.book['downloads'] = int(match[1].replace(',', ''))
                self.books.append(self.book)
            self.book = None


class Downloads(HTMLParser):
    def __init__(self, page):
        super().__init__()
        self.page = page
        self.epub = None

    def handle_starttag(self, tag, attrs):
        if tag == 'a':
            href = urljoin(self.page, dict(attrs).get('href', ''))
            if href == self.page + '.epub3.images':
                self.epub = href


class Fetcher:
    def __init__(self, delay):
        self.delay = delay
        self.lock = threading.Lock()
        self.previous = 0.0

    def fetch(self, url, path):
        if path.exists():
            return
        path.parent.mkdir(parents=True, exist_ok=True)
        for attempt in range(3):
            try:
                with self.lock:
                    time.sleep(max(0, self.delay - (time.monotonic() - self.previous)))
                    self.previous = time.monotonic()
                with urlopen(Request(url, headers={'User-Agent': 'bokheim-catalog'}), timeout=45) as response:
                    temporary = path.with_suffix(path.suffix + '.partial')
                    with temporary.open('wb') as destination:
                        shutil.copyfileobj(response, destination, 512 * 1024)
                    receipt = {'url': url, 'response_url': response.url, 'observed_at': now(), 'headers': dict(response.headers), 'bytes': temporary.stat().st_size}
                receipt['sha256'] = hashlib.sha256(temporary.read_bytes()).hexdigest()
                temporary.replace(path)
                write_json(path.with_suffix(path.suffix + '.http.json'), receipt)
                return
            except Exception:
                if attempt == 2:
                    raise
                time.sleep(2 ** (attempt + 1))


def is_britannica(title):
    normalized = unicodedata.normalize('NFKD', title.casefold()).replace('æ', 'ae')
    normalized = re.sub(r'[^a-z0-9]+', ' ', normalized)
    return re.search(r'\bencyclop(?:ae|e)dia britannica\b', normalized) is not None


def is_nuremberg_trials(title, subjects=()):
    def normalize(value):
        return ''.join(character for character in unicodedata.normalize('NFKD', value.casefold()) if not unicodedata.combining(character))
    if normalize(title).startswith('trial of the major war criminals before the international military tribunal'):
        return True
    city = re.compile(r'\b(?:nuremberg|nuremburg|nuernberg|nurnberg)(?:er)?\b')
    proceedings = re.compile(r'\b(?:trials?|tribunals?|proze\w*|proces\w*)\b')
    return any(city.search(normalize(value)) and proceedings.search(normalize(value)) for value in (title, *subjects))


def choose_books(candidates, limit, exclude_britannica=False, exclude_nuremberg_trials=False):
    ordered = sorted(candidates, key=lambda item: (-item[0]['downloads'], int(item[0]['entry_id'])))
    eligible, excluded = [], []
    for original, raw in ordered:
        row = dict(original)
        if exclude_nuremberg_trials and is_nuremberg_trials(row['title'], row.get('subjects', ())):
            excluded.append({**row, 'reason': 'Nuremberg trials'})
        elif exclude_britannica and is_britannica(row['title']):
            excluded.append({**row, 'reason': 'Encyclopaedia Britannica'})
        else:
            eligible.append((row, raw))
    return eligible[:limit], excluded, len(eligible)


def prepare(args):
    args.output.mkdir(parents=True, exist_ok=True)
    listing = f'{ORIGIN}/ebooks/bookshelf/{args.bookshelf}?sort_order=downloads'
    fetcher = Fetcher(args.delay)
    category, slug = CATEGORIES[args.bookshelf]
    archive = args.output / 'rdf-files.tar.bz2'
    metadata_url = ORIGIN + '/cache/epub/feeds/rdf-files.tar.bz2'
    print('Fetching the daily RDF catalog', flush=True)
    fetcher.fetch(metadata_url, archive)
    receipt = json.loads(archive.with_suffix('.bz2.http.json').read_text())
    download_metadata = {'method': 'daily_rdf_download_count', 'url': metadata_url, 'sha256': receipt['sha256'], 'last_modified': next((value for key, value in receipt['headers'].items() if key.lower() == 'last-modified'), None), 'observed_at': receipt['observed_at'], 'tie_break': 'ascending_gutenberg_id'}
    ns = {'pg': 'http://www.gutenberg.org/2009/pgterms/', 'dc': 'http://purl.org/dc/terms/', 'rdf': 'http://www.w3.org/1999/02/22-rdf-syntax-ns#'}
    candidates = []
    scanned = 0
    with tarfile.open(archive, mode='r|bz2') as catalog:
        for member in catalog:
            if not member.isfile() or not member.name.endswith('.rdf'):
                continue
            scanned += 1
            raw = catalog.extractfile(member).read()
            book = ET.fromstring(raw).find('pg:ebook', ns)
            if book is None or not any(node.text == 'Category: ' + category for node in book.findall('pg:bookshelf/rdf:Description/rdf:value', ns)):
                continue
            entry_id = book.attrib['{' + ns['rdf'] + '}about'].split('/')[-1]
            downloads = book.find('pg:downloads', ns)
            if not entry_id.isdigit() or downloads is None:
                raise ValueError('Category entry has no Gutenberg ID or download count')
            row = {'entry_id': entry_id, 'title': ' '.join(book.findtext('dc:title', default='', namespaces=ns).split()), 'author': ', '.join(node.text or '' for node in book.findall('dc:creator/pg:agent/pg:name', ns)), 'downloads': int(downloads.text), 'listing_url': listing, 'observed_at': receipt['observed_at'], 'page_url': f'{ORIGIN}/ebooks/{entry_id}', 'download_metadata': download_metadata}
            row['subjects'] = [node.text or '' for node in book.findall('dc:subject/rdf:Description/rdf:value', ns)]
            candidates.append((row, raw))
            if len(candidates) % 200 == 0:
                print(f'Found {len(candidates)} category books while scanning {scanned} RDF records', flush=True)
    selected, excluded, eligible_count = choose_books(candidates, args.limit, args.exclude_britannica, args.exclude_nuremberg_trials)
    rows = []
    for row, raw in selected:
        rows.append(row)
        path = args.output / 'metadata' / (row['entry_id'] + '.rdf')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
    if len(rows) != args.limit:
        raise ValueError(f'The category supplied {len(rows)} books, fewer than the requested {args.limit}')
    write_json(args.output / 'selection.json', {'bookshelf': args.bookshelf, 'category': category, 'slug': slug, 'listing_url': listing, 'limit': args.limit, 'category_books': len(candidates), 'eligible_books': eligible_count, 'exclusions': {'encyclopaedia_britannica': args.exclude_britannica, 'nuremberg_trials': args.exclude_nuremberg_trials, 'books': excluded}, 'selected_at': now(), 'download_metadata': download_metadata, 'books': rows})
    print(json.dumps({'selected': len(rows), 'first': rows[0], 'last': rows[-1]}, ensure_ascii=False, indent=2))


def check_storage(directory):
    if shutil.disk_usage(directory).free < 512 * 1024 * 1024:
        raise RuntimeError('Less than 512 MiB free; acquisition stopped without discarding progress')


def validate_epub3(path, entry_id=None):
    with zipfile.ZipFile(path) as archive:
        if archive.read('mimetype').strip() != b'application/epub+zip':
            raise ValueError('Download is not an EPUB')
        container = ET.fromstring(archive.read('META-INF/container.xml'))
        rootfile = next(node for node in container.iter() if node.tag.split('}')[-1] == 'rootfile')
        package = ET.fromstring(archive.read(rootfile.attrib['full-path']))
        if package.attrib.get('version') != '3.0':
            raise ValueError('Advertised EPUB3 has a different package version')
        if entry_id is not None:
            identities = [urlparse(node.text or '') for node in package.iter() if node.tag.split('}')[-1] == 'identifier']
            if not any(identity.hostname in ('www.gutenberg.org', 'gutenberg.org') and identity.path.rstrip('/') in ('/' + entry_id, '/ebooks/' + entry_id) for identity in identities):
                raise ValueError('Mirror EPUB does not identify the selected Gutenberg book')


def mirror_urls(advertised):
    match = re.fullmatch(re.escape(ORIGIN) + r'/ebooks/(\d+)\.epub3\.images', advertised)
    if not match:
        raise ValueError('Mirror delivery requires the page-advertised Gutenberg EPUB3 URL')
    identity = match[1]
    filename = f'{identity}/pg{identity}-images-3.epub'
    return ['https://mirror.cs.odu.edu/gutenberg-epub/' + filename, 'https://gutenberg.pglaf.org/cache/epub/' + filename]


def quarantine_download(path):
    if not path.exists():
        return
    rejected = path.parent.parent / 'rejected'
    rejected.mkdir(parents=True, exist_ok=True)
    name = path.name + '.' + str(time.time_ns())
    path.replace(rejected / name)
    receipt = path.with_suffix(path.suffix + '.http.json')
    if receipt.exists():
        receipt.replace(rejected / (name + '.http.json'))


def fetch_epub3(advertised, path, fetcher, entry_id):
    if path.exists():
        try:
            validate_epub3(path, entry_id)
            return json.loads(path.with_suffix(path.suffix + '.http.json').read_text())
        except Exception:
            quarantine_download(path)
    errors = []
    for url in mirror_urls(advertised):
        try:
            fetcher.fetch(url, path)
            validate_epub3(path, entry_id)
            receipt_path = path.with_suffix(path.suffix + '.http.json')
            receipt = json.loads(receipt_path.read_text())
            receipt['advertised_url'] = advertised
            receipt['epub_version'] = '3.0'
            receipt['gutenberg_id'] = entry_id
            write_json(receipt_path, receipt)
            return receipt
        except Exception as error:
            errors.append(f'{url}: {error}')
            quarantine_download(path)
            logging.warning('Mirror download failed: %s', errors[-1])
    raise RuntimeError('; '.join(errors))


def fetch_book_page(page, path, fetcher):
    try:
        fetcher.fetch(page, path)
    except HTTPError as error:
        if error.code not in (502, 503, 504):
            raise
        logging.warning('Book page returned HTTP %s; retrying the same page with a query parameter: %s', error.code, page)
        fetcher.fetch(page + '?source=bokheim', path)


def acquire(row, directory, fetcher, existing=None):
    check_storage(directory)
    page_file = directory / 'pages' / (row['entry_id'] + '.html')
    fetch_book_page(row['page_url'], page_file, fetcher)
    parsed = Downloads(row['page_url'])
    parsed.feed(page_file.read_text())
    if not parsed.epub:
        raise ValueError('The selected book page advertises no EPUB3')
    temporary_epub = None
    if existing and existing['epub_url'] == parsed.epub:
        epub = Path(existing['file'])
        validate_epub3(epub, row['entry_id'])
        receipt = {'response_url': existing.get('delivery_url')}
    else:
        epub = directory / 'spool' / (row['entry_id'] + '.epub')
        receipt = fetch_epub3(parsed.epub, epub, fetcher, row['entry_id'])
        temporary_epub = epub
    manifest = directory / 'entries' / (row['entry_id'] + '.json')
    write_json(manifest, [{'page_url': row['page_url'], 'page_file': str(page_file), 'file': str(epub), 'file_url': parsed.epub, 'delivery_url': receipt['response_url'], 'downloads': row['downloads'], 'listing_url': row['listing_url'], 'observed_at': row['observed_at'], 'download_metadata': row.get('download_metadata')}])
    return manifest, temporary_epub


def reuse_existing(catalog, directory, books, db):
    """Reuse stored EPUB3s and retained pages without deleting catalog assets."""
    existing = {}
    sources = {entry_id: (book_hash, json.loads(metadata)) for entry_id, book_hash, metadata in db.execute("SELECT entry_id,book_hash,metadata FROM source_entry WHERE source='gutenberg.org' AND book_hash IS NOT NULL")}
    for row in books:
        if row['entry_id'] not in sources:
            continue
        book_hash, evidence = sources[row['entry_id']]
        epub = catalog / 'books' / (book_hash + '.epub')
        if not epub.is_file() or evidence.get('epub_url') != row['page_url'] + '.epub3.images':
            continue
        existing[row['entry_id']] = {**evidence, 'file': str(epub)}
        target = directory / 'pages' / (row['entry_id'] + '.html')
        if target.exists():
            continue
        for source in sorted(directory.parent.glob('*/pages/' + target.name)):
            if source == target:
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
            receipt = source.with_suffix('.html.http.json')
            if receipt.exists():
                shutil.copyfile(receipt, target.with_suffix('.html.http.json'))
            break
    return existing


def publish(args, directory):
    for _ in range(5):
        releases = args.catalog / 'published' / 'releases'
        release = max((int(path.name) for path in releases.iterdir() if path.name.isdigit()), default=0) + 1
        result = subprocess.run([str(args.binary), str(args.catalog), 'publish', str(args.catalog / 'published'), str(release)], capture_output=True, text=True)
        if result.returncode:
            logging.warning('Publish retry: %s', result.stderr[-2000:])
            time.sleep(2)
            continue
        snapshot = releases / str(release) / 'catalog.sqlite3'
        with sqlite3.connect(f'file:{snapshot}?mode=ro', uri=True) as db:
            if db.execute('PRAGMA integrity_check').fetchone()[0] != 'ok' or db.execute('PRAGMA foreign_key_check').fetchall():
                raise ValueError('Published snapshot failed integrity checks')
        installed = args.catalog.parent / 'catalog.sqlite3'
        temporary = installed.with_suffix('.sqlite3.gutenberg-partial')
        shutil.copyfile(snapshot, temporary)
        temporary.replace(installed)
        write_json(directory / 'last-publication.json', {'release_id': release, 'snapshot': str(snapshot), 'installed': str(installed), 'published_at': now()})
        logging.info('Installed local catalog release %s', release)
        return
    raise RuntimeError('Could not publish a local snapshot after five attempts')


def wait_for_selection(catalog, selection_path):
    previous = json.loads(selection_path.read_text())
    slug = CATEGORIES[previous['bookshelf']][1]
    directory = catalog / 'imports' / 'gutenberg.org' / (slug + '-' + previous['selected_at'].replace(':', '').replace('+', '_'))
    logging.info('Waiting for the preceding %s selection to complete', previous.get('category', slug))
    while True:
        progress = directory / 'progress.json'
        if progress.exists() and json.loads(progress.read_text()).get('state') == 'completed':
            return
        time.sleep(30)


def run(args):
    selection = json.loads(args.selection.read_text())
    slug = CATEGORIES[selection['bookshelf']][1]
    directory = args.catalog / 'imports' / 'gutenberg.org' / (slug + '-' + selection['selected_at'].replace(':', '').replace('+', '_'))
    directory.mkdir(parents=True, exist_ok=True)
    if (args.selection.parent / 'listings').exists():
        shutil.copytree(args.selection.parent / 'listings', directory / 'listings', dirs_exist_ok=True)
    shutil.copytree(args.selection.parent / 'metadata', directory / 'metadata', dirs_exist_ok=True)
    write_json(directory / 'selection.json', selection)
    logging.basicConfig(level=logging.INFO, format='%(asctime)s %(levelname)s %(message)s', handlers=[logging.StreamHandler(), logging.FileHandler(directory / 'ingest.log')])
    if args.after_selection:
        wait_for_selection(args.catalog, args.after_selection)
    lock = (args.catalog / 'gutenberg-import.lock').open('a')
    logging.info('Waiting for the catalog Gutenberg import lock')
    fcntl.flock(lock, fcntl.LOCK_EX)
    progress = directory / 'progress.json'
    state = json.loads(progress.read_text()) if progress.exists() else {'selected': len(selection['books']), 'started_at': now(), 'outcomes': {}}
    with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db:
        if not (directory / 'curation-before.sqlite3').exists():
            with sqlite3.connect(directory / 'curation-before.sqlite3') as backup:
                db.backup(backup)
        state.setdefault('initial_hosted_books', db.execute('SELECT count(*) FROM book').fetchone()[0])
        existing = reuse_existing(args.catalog, directory, selection['books'], db)
        state['reusable_books'] = len(existing)
    fetcher = Fetcher(args.delay)
    pending = [row for row in selection['books'] if state['outcomes'].get(row['entry_id'], {}).get('state') != 'imported']
    pending.sort(key=lambda row: (not (directory / 'pages' / (row['entry_id'] + '.html')).exists(), -row['downloads'], int(row['entry_id'])))
    state['state'] = 'running'
    state.pop('finished_at', None)
    write_json(progress, state)
    logging.info('Importing %s pending books from a frozen selection of %s via official EPUB3 mirrors; cached pages first; evidence: %s', len(pending), len(selection['books']), directory)
    successes = sum(value['state'] == 'imported' for value in state['outcomes'].values())
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        tasks = deque()
        iterator = iter(pending)
        for row in list(pending[:args.workers]):
            next(iterator)
            tasks.append((row, pool.submit(acquire, row, directory, fetcher, existing.get(row['entry_id']))))
        while tasks:
            row, future = tasks.popleft()
            try:
                manifest, epub = future.result()
                check_storage(args.catalog)
                for attempt in range(3):
                    result = subprocess.run([str(args.binary), str(args.catalog), 'import-gutenberg', str(manifest)], capture_output=True, text=True, timeout=240)
                    if result.returncode == 0:
                        break
                    if attempt < 2:
                        time.sleep(2 ** (attempt + 1))
                if result.returncode:
                    raise RuntimeError(result.stderr[-4000:])
                with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db:
                    stored = db.execute("SELECT s.book_hash,s.metadata,b.record FROM source_entry s JOIN book b ON b.hash=s.book_hash WHERE s.source='gutenberg.org' AND s.entry_id=?", (row['entry_id'],)).fetchone()
                if not stored:
                    raise ValueError('Successful import omitted its source evidence')
                evidence, record = json.loads(stored[1]), json.loads(stored[2])
                identifiers = {re.sub(r'[-\s]', '', item['value']) for item in record['book']['identifiers'] if item['scheme'] == 'isbn'}
                if any(item['value'] not in identifiers for item in evidence['source_metadata']['identifiers']):
                    raise ValueError('Source ISBN was lost during import')
                if any(subject not in record['book']['subjects'] for subject in evidence['source_metadata']['subjects']):
                    raise ValueError('Original subject evidence was lost during import')
                state['outcomes'][row['entry_id']] = {'state': 'imported', 'hash': stored[0], 'title': record['title'], 'year': record['book']['year'], 'page_year': evidence['source_metadata']['year'], 'reused': epub is None, 'completed_at': now()}
                if epub is not None:
                    epub.unlink()
                successes += 1
                logging.info('%s/%s imported: %s downloads, %s (%s)', successes, len(selection['books']), row['downloads'], record['title'], stored[0][:12])
                if successes == 10 or successes % args.publish_every == 0:
                    try:
                        publish(args, directory)
                    except Exception as error:
                        logging.warning('Snapshot installation will retry: %s', error)
            except Exception as error:
                state['outcomes'][row['entry_id']] = {'state': 'failed', 'error': str(error), 'updated_at': now()}
                logging.error('Book %s failed: %s', row['entry_id'], error)
                if shutil.disk_usage(args.catalog).free < 512 * 1024 * 1024:
                    state['state'] = 'waiting_for_space'
                    write_json(progress, state)
                    raise
            state['imported'] = successes
            state['failed'] = sum(value['state'] == 'failed' for value in state['outcomes'].values())
            state['updated_at'] = now()
            write_json(progress, state)
            try:
                row = next(iterator)
                tasks.append((row, pool.submit(acquire, row, directory, fetcher, existing.get(row['entry_id']))))
            except StopIteration:
                pass
    publish(args, directory)
    state['state'] = 'completed' if successes == len(selection['books']) else 'completed_with_failures'
    state['finished_at'] = now()
    write_json(progress, state)
    logging.info('Finished: %s imported, %s failed', successes, state.get('failed', 0))
    if state['state'] != 'completed':
        raise SystemExit(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    prepare_parser = sub.add_parser('prepare')
    prepare_parser.add_argument('--output', type=Path, required=True)
    prepare_parser.add_argument('--bookshelf', type=int, choices=CATEGORIES, default=691)
    prepare_parser.add_argument('--limit', type=int, default=1200)
    prepare_parser.add_argument('--exclude-britannica', action='store_true')
    prepare_parser.add_argument('--exclude-nuremberg-trials', action='store_true')
    prepare_parser.add_argument('--delay', type=float, default=1)
    run_parser = sub.add_parser('run')
    run_parser.add_argument('--catalog', type=Path, required=True)
    run_parser.add_argument('--selection', type=Path, required=True)
    run_parser.add_argument('--binary', type=Path, required=True)
    run_parser.add_argument('--after-selection', type=Path)
    run_parser.add_argument('--delay', type=float, default=0)
    run_parser.add_argument('--workers', type=int, default=3)
    run_parser.add_argument('--publish-every', type=int, default=100)
    args = parser.parse_args()
    if args.delay < 0 or getattr(args, 'workers', 1) < 1 or getattr(args, 'publish_every', 1) < 1:
        parser.error('delay must be nonnegative; worker and publication counts must be positive')
    (prepare if args.command == 'prepare' else run)(args)


if __name__ == '__main__':
    main()
