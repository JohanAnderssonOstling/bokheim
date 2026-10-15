#!/usr/bin/env python3
"""Validate cached 48-book popularity pages and build an observed ordinal ranking."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
from urllib.parse import parse_qs, urljoin, urlparse
import xml.etree.ElementTree as ET

LISTING = 'https://standardebooks.org/ebooks?sort=popularity&per-page=48'


def ranking(directory):
    entries, pages, seen = [], [], set()
    files = sorted(directory.glob('page-*.xhtml'))
    if not files:
        raise ValueError('No cached popularity pages')
    expected_pages = None
    for number, path in enumerate(files, 1):
        if path.name != f'page-{number:02}.xhtml':
            raise ValueError('A popularity page is missing')
        data = path.read_bytes()
        root = ET.fromstring(data)
        sort = root.find('.//{*}select[@name="sort"]/{*}option[@selected="selected"]')
        per_page = root.find('.//{*}select[@name="per-page"]/{*}option[@selected="selected"]')
        if sort is None or sort.get('value') != 'popularity' or per_page is None or per_page.get('value') != '48':
            raise ValueError('Expected popularity ordering with 48 books per page')
        if number == 1:
            numbers = [1]
            for link in root.findall('.//{*}a'):
                query = parse_qs(urlparse(link.get('href', '')).query)
                if query.get('sort') == ['popularity'] and query.get('per-page') == ['48']:
                    numbers.extend(int(p) for p in query.get('page', []) if p.isdigit())
            expected_pages = max(numbers)
            if len(files) != expected_pages:
                raise ValueError('The cached pages do not cover the complete pagination')
        books = root.findall('.//{*}li[@typeof="schema:Book"]')
        if not books or len(books) > 48 or (number < expected_pages and len(books) != 48):
            raise ValueError('Unexpected number of books on a popularity page')
        url = LISTING + f'&page={number}'
        pages.append({'page': number, 'url': url, 'books': len(books), 'sha256': hashlib.sha256(data).hexdigest()})
        for book in books:
            identity = book.get('about', '')
            if not identity.startswith('/ebooks/'):
                raise ValueError('Missing source book identity')
            identity = urljoin('https://standardebooks.org', identity)
            if identity in seen:
                raise ValueError('A book occurs on multiple popularity pages; fetch a fresh consistent set')
            seen.add(identity)
            title = book.find('./{*}p/{*}a/{*}span[@property="schema:name"]')
            authors = book.findall('.//{*}p[@property="schema:author"]/{*}a/{*}span[@property="schema:name"]')
            if title is None:
                raise ValueError('Missing listing title')
            entries.append({'entry_id': identity, 'rank': len(entries) + 1,
                            'title': ''.join(title.itertext()).strip(),
                            'authors': [''.join(author.itertext()).strip() for author in authors]})
    return {'source': 'standardebooks.org', 'observed_at': datetime.now(timezone.utc).isoformat(timespec='seconds'),
            'listing_url': LISTING, 'per_page': 48, 'total': len(entries), 'pages': pages, 'entries': entries,
            'measurement': 'Published order by non-bot downloads over the past 30 days; numeric counts unavailable'}


def apply(db, manifest):
    if manifest['source'] != 'standardebooks.org' or manifest['per_page'] != 48:
        raise ValueError('Unexpected ranking source or page size')
    entries = manifest['entries']
    if [entry['rank'] for entry in entries] != list(range(1, manifest['total'] + 1)):
        raise ValueError('Popularity ranks must be a complete contiguous ordering')
    by_id = {entry['entry_id']: entry for entry in entries}
    if len(by_id) != len(entries):
        raise ValueError('Popularity identities must be unique')
    targets = [row[0] for row in db.execute("SELECT entry_id FROM source_entry WHERE source='standardebooks.org' AND book_hash IS NOT NULL")]
    if not targets or set(targets) - by_id.keys():
        raise ValueError('The ranking omits an imported Standard Ebooks source entry')
    for identity in targets:
        evidence = {'rank': by_id[identity]['rank'], 'total': manifest['total'], 'kind': 'ordinal', 'scope': 'sitewide',
                    'observed_at': manifest['observed_at'], 'listing_url': manifest['listing_url'],
                    'measurement': manifest['measurement']}
        db.execute("UPDATE source_entry SET metadata=json_set(metadata,'$.popularity',json(?)) WHERE source='standardebooks.org' AND entry_id=?",
                   (json.dumps(evidence), identity))
    return len(targets)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pages', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = ranking(args.pages)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'books': result['total'], 'pages': len(result['pages']), 'per_page': result['per_page']}))
