#!/usr/bin/env python3
"""Prepare category selections, hold uncertain books for review, acquire approved files.

Uses only Python's standard library. Preparation fetches metadata, never books.
Downloads stay local; EPUBs can then enter the existing import-manifest command.
"""
import argparse
from collections import Counter
import csv
from datetime import datetime, timezone
import fcntl
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import shutil
import time
import uuid
from urllib.parse import urljoin, urlparse, urlunparse, unquote
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET
import zipfile

ORIGIN = 'https://unglue.it'
NS = {'a': 'http://www.w3.org/2005/Atom', 'd': 'http://purl.org/dc/terms/'}
FORMATS = {'application/epub+zip': 'epub', 'application/pdf': 'pdf'}
TECH = re.compile(r'programming|software|comput(?:er|ing)|linux|ubuntu|python|javascript|'
                  r'android|\brails\b|\bphp\b|\bjava\b|operating systems|web development|'
                  r'cybersecurity|network security|机器学习|计算机|编程', re.I)
SENSITIVE = re.compile(r'medicine|medical|clinical|legal|legislation|taxation|'
                       r'financial advice|investment guide', re.I)
VERSION = re.compile(r'\b(?:ubuntu|debian|windows|android|rails|python|php|java|'
                     r'javascript|django|drupal|wordpress)\s+(?:v(?:ersion)?\s*)?\d', re.I)


class ExcludedOrigin(ValueError):
    pass


def now():
    return datetime.now(timezone.utc).isoformat(timespec='seconds')


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


def read_json(path):
    return json.loads(path.read_text())


def write_review_csv(directory, books):
    path = directory / 'review.csv'
    temporary = path.with_suffix('.csv.partial')
    fields = ['entry_id', 'title', 'authors', 'year', 'faves', 'downloads', 'formats',
              'isbns', 'dois', 'website', 'openlibrary', 'librarything', 'reason']
    with temporary.open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        for row in sorted((r for r in books if r['decision'] == 'review'),
                          key=lambda r: (r.get('faves') is None, -(r.get('faves') or 0), r['title'])):
            website = row.get('website_metadata', {})
            identifiers = website.get('identifiers', [])
            providers = website.get('provider_links', {})
            writer.writerow(dict(entry_id=row['entry_id'], title=row['title'], authors='; '.join(row['authors']),
                year=row['year'], faves=row['faves'], downloads=website.get('downloads'),
                formats='; '.join(sorted({a['format'] for a in row['acquisitions']})),
                isbns='; '.join(sorted({i['canonical'] for i in identifiers if i['scheme'] == 'isbn' and i['canonical']})),
                dois='; '.join(sorted({i['value'] for i in identifiers if i['scheme'] == 'doi'})),
                website=row['entry_url'], openlibrary='; '.join(sorted({link['url'] for link in providers.get('openlibrary', [])})),
                librarything='; '.join(sorted({link['url'] for link in providers.get('librarything', [])})), reason=row['reason']))
    temporary.replace(path)


def http_url(url):
    parsed = urlparse(url)
    if parsed.scheme not in ('http', 'https') or not parsed.hostname or parsed.username:
        raise ValueError('Invalid HTTP URL: ' + url)
    return url


def gutenberg_url(url):
    parsed = urlparse(url)
    host = (parsed.hostname or '').lower()
    return (host == 'gutenberg.org' or host.endswith('.gutenberg.org') or
            host == 'gitenberg.org' or host.endswith('.gitenberg.org') or
            host == 'github.com' and parsed.path.lower().startswith('/gitenberg/'))


def category_feed(url):
    parsed = urlparse(http_url(url))
    if parsed.hostname not in ('unglue.it', 'www.unglue.it'):
        raise ValueError('Expected an Unglue.it category URL')
    if parsed.path.startswith('/free/'):
        path = '/api/opds/' + parsed.path[len('/free/'):]
    elif parsed.path.startswith('/api/opds/'):
        path = parsed.path
    else:
        raise ValueError('Expected /free/ or /api/opds/ category URL')
    # The live API rejects every compound keyword facet (#1095). Keyword
    # categories therefore use the website/metadata/redirect origin checks.
    if not any(part.startswith('kw.') for part in path.split('/')) and '-gtbg' not in path.split('/'):
        path = path.rstrip('/') + '/-gtbg/'
    return urlunparse(('https', 'unglue.it', path, '', parsed.query, ''))


class HTML(HTMLParser):
    def __init__(self, value):
        super().__init__()
        self.parts = []
        self.links = []
        self.hidden = 0
        self.feed(value)

    def handle_starttag(self, tag, attrs):
        if tag in ('script', 'style'):
            self.hidden += 1
        if tag == 'a':
            self.links.append(dict(attrs).get('href', ''))

    def handle_endtag(self, tag):
        if tag in ('script', 'style'):
            self.hidden = max(0, self.hidden - 1)

    def handle_data(self, value):
        if not self.hidden:
            self.parts.append(value)

    def text(self):
        return ' '.join(' '.join(self.parts).split())


class Node:
    def __init__(self, tag='', attrs=None):
        self.tag, self.attrs, self.children = tag, attrs or {}, []

    def walk(self):
        yield self
        for child in self.children:
            if isinstance(child, Node):
                yield from child.walk()

    def text(self):
        return ''.join(child.text() if isinstance(child, Node) else child for child in self.children)

    def clean(self):
        return ' '.join(self.text().split())


class Document(HTMLParser):
    def __init__(self, page):
        super().__init__()
        self.root = Node()
        self.stack = [self.root]
        self.feed(page)

    def handle_starttag(self, tag, attrs):
        node = Node(tag, dict(attrs))
        self.stack[-1].children.append(node)
        if tag == 'br':
            node.children.append('\u2028')
        if tag not in ('area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'):
            self.stack.append(node)

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        self.handle_endtag(tag)

    def handle_endtag(self, tag):
        for index in range(len(self.stack) - 1, 0, -1):
            if self.stack[index].tag == tag:
                del self.stack[index:]
                break

    def handle_data(self, value):
        self.stack[-1].children.append(value)


def valid_isbn(value):
    value = re.sub(r'[\s-]', '', value).upper()
    if re.fullmatch(r'\d{9}[\dX]', value):
        valid = sum((10 - i) * (10 if c == 'X' else int(c)) for i, c in enumerate(value)) % 11 == 0
    elif re.fullmatch(r'97[89]\d{10}', value):
        valid = sum(int(c) * (1 if i % 2 == 0 else 3) for i, c in enumerate(value)) % 10 == 0
    else:
        valid = False
    return value if valid else None


def page_metadata(page, base):
    nodes = list(Document(page).root.walk())
    def properties(name):
        return [node for node in nodes if name in node.attrs.get('itemprop', '').split()]
    def links(group):
        return [dict(url=urljoin(base, node.attrs['href']), label=node.clean(),
                     rel=node.attrs.get('rel')) for node in group if node.tag == 'a' and node.attrs.get('href')]
    editions, identifiers = [], []
    edition_nodes = [node for node in nodes if node.attrs.get('id', '').startswith('edition_')]
    edition_members = {id(node) for edition in edition_nodes for node in edition.walk()}
    for edition in edition_nodes:
        fields = {}
        for line in edition.text().split('\u2028'):
            line = ' '.join(line.split())
            match = re.match(r'(Contributors|Publisher|Published|ISBN|DOI|OCLC|LCCN|ISSN):\s*(.+)', line, re.I)
            if match:
                fields.setdefault(match[1].lower(), []).append(match[2])
        ebook_match = re.search(r'\b(\d+)\s+ebooks?\b', edition.clean())
        edition_id = edition.attrs['id'].removeprefix('edition_')
        info = dict(edition_id=edition_id, fields=fields, links=links(edition.walk()),
                    ebook_count=int(ebook_match[1]) if ebook_match else 0, text=edition.clean())
        editions.append(info)
    def identifier(scheme, value, edition_id=None, downloadable=False):
        value = value.strip()
        canonical = valid_isbn(value) if scheme == 'isbn' else value
        item = dict(scheme=scheme, value=value, canonical=canonical,
                    scope='edition' if edition_id else 'work', edition_id=edition_id,
                    downloadable_edition=downloadable)
        if item not in identifiers:
            identifiers.append(item)
    for edition in editions:
        for scheme in ('isbn', 'doi', 'oclc', 'lccn', 'issn'):
            for value in edition['fields'].get(scheme, []):
                identifier(scheme, value, edition['edition_id'], edition['ebook_count'] > 0)
    for node in nodes:
        if id(node) in edition_members:
            continue
        for scheme in ('isbn', 'doi'):
            if scheme in node.attrs.get('itemprop', '').lower().split():
                identifier(scheme, node.attrs.get('content') or node.clean())
        if node.tag == 'meta':
            scheme = {'citation_isbn': 'isbn', 'citation_doi': 'doi'}.get(node.attrs.get('name', '').lower())
            if scheme and node.attrs.get('content'):
                identifier(scheme, node.attrs['content'])
    all_links = links(nodes)
    for link in all_links:
        parsed = urlparse(link['url'])
        if parsed.hostname in ('doi.org', 'dx.doi.org'):
            # Retain DOI links even when their surrounding field is unrecognized.
            doi = unquote(parsed.path.lstrip('/'))
            if not any(item['scheme'] == 'doi' and item['value'].removeprefix('https://doi.org/') == doi for item in identifiers):
                owner = next((edition for edition in edition_nodes if any(
                    urljoin(base, n.attrs.get('href', '')) == link['url'] for n in edition.walk() if n.tag == 'a')), None)
                identifier('doi', doi, owner.attrs['id'].removeprefix('edition_') if owner else None)
    full_text = HTML(page).text()
    downloads = re.search(r'This work has been downloaded\s+([\d,]+)\s+times', full_text, re.I)
    breakdown = []
    for node in nodes:
        if node.tag == 'li' and re.match(r'^[\d,]+\s*-\s*(?:epub|pdf|mobi)\b', node.clean(), re.I):
            match = re.match(r'^([\d,]+)\s*-\s*(\w+)\s*(.*)', node.clean())
            breakdown.append(dict(count=int(match[1].replace(',', '')), format=match[2].lower(), details=match[3]))
    provider_links = {}
    for provider in ('openlibrary', 'librarything', 'worldcat', 'google'):
        provider_links[provider] = [link for link in all_links if provider in link['url'].lower()]
    issued = properties('datePublished')
    date = issued[0].clean() if issued else ''
    if not properties('name'):
        raise ValueError('Work page has no book title')
    description_members = {id(node) for description in properties('description') for node in description.walk()}
    provenance_links = links(node for node in nodes if id(node) not in description_members)
    origins = [dict(kind='work_link', value=link['url']) for link in provenance_links
               if gutenberg_url(link['url']) and re.search(r'/(?:ebooks|files)/\d+', urlparse(link['url']).path)]
    for edition in editions:
        for publisher in edition['fields'].get('publisher', []):
            if re.search(r'\b(?:Project Gutenberg|GITenberg)\b', publisher, re.I):
                origins.append(dict(kind='edition_publisher', value=publisher, edition_id=edition['edition_id']))
    if re.search(r'This book is included in Project Gutenberg\b', full_text, re.I):
        origins.append(dict(kind='website_collection', value='Project Gutenberg'))
    return dict(title=properties('name')[0].clean(),
                authors=[node.clean() for node in properties('author')],
                issued=date, year=int(date[:4]) if re.match(r'^\d{4}(?:$|[-/])', date) else None,
                languages=[node.clean() for node in properties('inLanguage') if node.clean()],
                description=properties('description')[0].clean() if properties('description') else '',
                subjects=[re.sub(r'\s+\(\d+\)$', '', node.clean()) for node in properties('keywords')],
                faves=fave_count(page), downloads=int(downloads[1].replace(',', '')) if downloads else None,
                download_breakdown=breakdown, identifiers=identifiers, editions=editions,
                provider_links=provider_links, links=all_links, gutenberg_origin=bool(origins), origin_evidence=origins)


def fave_count(page):
    # Downloads, campaign thanks and the OPDS schema:Rating are different counts.
    counts = {int(m.replace(',', '')) for m in re.findall(
        r'([\d,]+)\s+Ungluers?\s+ha(?:ve|s)\s+Faved this Work', HTML(page).text(), re.I)}
    if len(counts) == 1:
        return counts.pop()
    if counts:
        return None
    # The site's work template renders the same `wishers` value in this tab
    # and in "Faved this Work". Campaign pages omit the latter.
    # https://github.com/Gluejar/regluit/blob/master/frontend/templates/work.html
    tabs = [node.clean() for node in Document(page).root.walk() if node.attrs.get('id') == 'supporters']
    tab_counts = set()
    for tab in tabs:
        match = re.fullmatch(r'Ungluers(?:\s*\(([\d,]+)\))?', tab)
        if match:
            tab_counts.add(int(match[1].replace(',', '')) if match[1] else 0)
    if len(tab_counts) == 1:
        return tab_counts.pop()
    return None  # Missing or ambiguous counts must be reviewed, never guessed.


def download_page_acquisitions(page, base):
    """Read the website's freely available editions, including feed omissions."""
    groups = [node for node in Document(page).root.walk()
              if 'ebook_download' in node.attrs.get('class', '').split()]
    links = {}
    for group in groups:
        for node in group.walk():
            if node.tag != 'a' or not node.attrs.get('href'):
                continue
            url = urljoin(base, node.attrs['href'])
            if not re.fullmatch(r'/download_ebook/\d+/', urlparse(url).path):
                continue
            item = links.setdefault(url, dict(url=url, format=None, rights=None))
            filename = node.attrs.get('data-filename', '')
            if filename.endswith(('.epub', '.pdf', '.mobi')):
                item['format'] = filename.rsplit('.', 1)[1]
            match = re.match(r'^(epub|pdf|mobi)\b', node.clean(), re.I)
            if match:
                item['format'] = match[1].lower()
            for image in node.walk():
                if image.tag != 'img':
                    continue
                label = image.attrs.get('alt', '')
                match = re.match(r'^(epub|pdf|mobi)\b', label, re.I)
                if match:
                    item['format'] = match[1].lower()
                elif label:
                    item['rights'] = label
    return sorted((item for item in links.values() if item['format']),
                  key=lambda item: (item['format'] != 'epub', item['format'] != 'pdf', item['url']))


def has_gutenberg_origin(row):
    website = row.get('website_metadata', {})
    if row.get('gutenberg_origin') or website.get('gutenberg_origin'):
        return True
    if any(gutenberg_url(item['url']) for item in row.get('acquisitions', [])):
        return True
    return any(re.search(r'\b(?:Project Gutenberg|GITenberg)\b', item.get('details', ''), re.I)
               for item in website.get('download_breakdown', []))


class Fetcher:
    def __init__(self, delay):
        self.delay = delay
        self.previous = 0

    def fetch(self, url, path, redirect_resolver=None):
        http_url(url)
        receipt_path = path.with_suffix(path.suffix + '.http.json')
        if path.exists() and receipt_path.exists():
            receipt = read_json(receipt_path)
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            if url in (receipt['url'], receipt.get('requested_url')) and digest == receipt['sha256']:
                return receipt
            raise ValueError('Cached evidence changed or belongs to another URL: ' + str(path))
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(path.suffix + '.partial')
        for attempt in range(3):
            try:
                time.sleep(max(0, self.delay - (time.monotonic() - self.previous)))
                self.previous = time.monotonic()
                with urlopen(Request(url, headers={'User-Agent': 'bokheim-catalog'}), timeout=60) as response:
                    if redirect_resolver:
                        resolved = redirect_resolver(response.url)
                        if resolved != response.url:
                            response.close()
                            receipt = self.fetch(resolved, path)
                            receipt['requested_url'] = url
                            write_json(receipt_path, receipt)
                            return receipt
                    with temporary.open('wb') as destination:
                        shutil.copyfileobj(response, destination, 512 * 1024)
                    receipt = dict(url=url, response_url=response.url, observed_at=now(),
                                   headers=dict(response.headers), bytes=temporary.stat().st_size,
                                   sha256=hashlib.sha256(temporary.read_bytes()).hexdigest())
                temporary.replace(path)
                write_json(receipt_path, receipt)
                return receipt
            except ExcludedOrigin:
                temporary.unlink(missing_ok=True)
                raise
            except Exception:
                temporary.unlink(missing_ok=True)
                if attempt == 2:
                    raise
                time.sleep(2 ** attempt)


def parse_feed(raw, base):
    root = ET.fromstring(raw)
    if root.tag != '{' + NS['a'] + '}feed':
        raise ValueError('Response is not an Atom feed')
    rows = []
    for entry in root.findall('a:entry', NS):
        identity = entry.findtext('a:id', '', NS).strip()
        match = re.fullmatch(r'https?://(?:www\.)?unglue\.it/api/id/work/(\d+)/?', identity)
        if not match:
            raise ValueError('Unexpected work identity: ' + identity)
        issued = entry.findtext('d:issued', '', NS).strip()
        year = int(issued[:4]) if re.match(r'^\d{4}(?:$|[-/])', issued) else None
        acquisitions = []
        for link in entry.findall('a:link', NS):
            if link.get('rel') == 'http://opds-spec.org/acquisition/open-access' and link.get('type') in FORMATS:
                acquisitions.append(dict(url=http_url(urljoin(base, link.attrib['href'])),
                                         format=FORMATS[link.get('type')],
                                         rights=link.get('{' + NS['d'] + '}rights')))
        acquisitions.sort(key=lambda link: (link['format'] != 'epub', link['url']))
        provenance = [node.text.strip() for node in entry.findall('d:identifier', NS) if node.text]
        gutenberg_origin = any(gutenberg_url(value) or re.match(r'^(?:urn:)?(?:gtbg|gutenberg):', value, re.I) for value in provenance)
        rows.append(dict(entry_id=match[1], entry_url=f'{ORIGIN}/work/{match[1]}/',
                         title=' '.join(entry.findtext('a:title', '', NS).split()),
                         authors=[node.text.strip() for node in entry.findall('a:author/a:name', NS) if node.text],
                         year=year, issued=issued,
                         languages=[node.text.strip() for node in entry.findall('d:language', NS) if node.text],
                         description=HTML(entry.findtext('a:content', '', NS)).text(),
                         subjects=[node.get('term') for node in entry.findall('a:category', NS) if node.get('term')],
                         acquisitions=acquisitions, gutenberg_origin=gutenberg_origin))
    next_links = [urljoin(base, link.attrib['href']) for link in root.findall('a:link', NS)
                  if link.get('rel') == 'next']
    if len(next_links) > 1:
        raise ValueError('Ambiguous pagination')
    return rows, next_links[0] if next_links else None


def classify(row, year, review_age):
    if has_gutenberg_origin(row):
        return 'skip', 'Project Gutenberg origin excluded'
    faves = row.get('faves')
    if faves is None:
        return 'review', 'Fave count could not be verified; do not acquire yet'
    if faves < 1:
        return 'skip', 'No faves'
    title = row['title']
    obsolete = re.search(r'\bubuntu\b[^\d]{0,40}(\d{1,2})\.(\d{2})\b', title, re.I)
    if obsolete and (int(obsolete[1]), int(obsolete[2])) <= (14, 4):
        return 'skip', 'Clearly obsolete Ubuntu edition (14.04 or earlier)'
    if row.get('acquisition_error'):
        return 'review', 'Website download options could not be verified'
    if not row['acquisitions']:
        return 'review', 'No advertised open-access EPUB or PDF'
    text = ' '.join([title, *row['subjects'], *[unquote(u) for u in row['categories']]])
    technical = bool(TECH.search(text))
    sensitive = technical or bool(SENSITIVE.search(text))
    if VERSION.search(title):
        return 'review', 'Version-specific software book; assess continued usefulness'
    if sensitive and (row['year'] is None or row['year'] > year):
        return 'review', 'Age-sensitive subject with unknown or inconsistent publication year'
    if sensitive and year - row['year'] >= review_age:
        return 'review', f'Age-sensitive subject, published {row["year"]}; assess continued usefulness'
    return 'include', 'At least one fave; no freshness concern detected'


def prepare(args, fetcher):
    directory = args.output
    selection_path = directory / 'selection.json'
    if selection_path.exists():
        previous = read_json(selection_path)
        if previous['categories'] != args.categories or previous['review_age'] != args.review_age:
            raise ValueError('Use a new output directory for a different selection or age threshold')
    rows, errors = {}, []
    for category in args.categories:
        page, seen = category_feed(category), set()
        while page:
            if page in seen:
                errors.append(dict(url=page, error='Pagination loop; selection incomplete'))
                break
            seen.add(page)
            try:
                if not page.startswith(ORIGIN + '/api/opds/'):
                    raise ValueError('Pagination left the category feed API')
                path = directory / 'feeds' / (hashlib.sha256(page.encode()).hexdigest() + '.xml')
                fetcher.fetch(page, path)
                entries, next_page = parse_feed(path.read_bytes(), page)
                for row in entries:
                    if row['entry_id'] not in rows:
                        rows[row['entry_id']] = dict(row, categories=[], feed_urls=[])
                    stored = rows[row['entry_id']]
                    if category not in stored['categories']:
                        stored['categories'].append(category)
                    stored['feed_urls'].append(page)
                page = next_page
            except Exception as error:
                errors.append(dict(url=page, error=str(error)))
                break
    for row in rows.values():
        try:
            path = directory / 'pages' / (row['entry_id'] + '.html')
            receipt = fetcher.fetch(row['entry_url'], path)
            website = page_metadata(path.read_text(), row['entry_url'])
            row['website_metadata'] = website
            # Keep feed metadata as separate evidence; website facts drive selection.
            row['feed_metadata'] = {key: row[key] for key in ('title', 'authors', 'year', 'issued', 'languages', 'description', 'subjects')}
            for key in ('title', 'authors', 'year', 'issued', 'languages', 'description', 'subjects', 'faves'):
                row[key] = website[key]
            row['observed_at'] = receipt['observed_at']
            obsolete = re.search(r'\bubuntu\b[^\d]{0,40}(\d{1,2})\.(\d{2})\b', row['title'], re.I)
            if row['faves'] and not (obsolete and (int(obsolete[1]), int(obsolete[2])) <= (14, 4)):
                download_url = row['entry_url'] + 'download/'
                download_path = directory / 'download-pages' / (row['entry_id'] + '.html')
                try:
                    fetcher.fetch(download_url, download_path)
                    links = download_page_acquisitions(download_path.read_text(), download_url)
                    website['acquisitions'] = links
                    advertised = [link for link in links if link['format'] in ('epub', 'pdf')]
                    if advertised:
                        row['acquisitions'] = advertised
                except Exception as error:
                    row['acquisition_error'] = str(error)
        except Exception as error:
            row['faves'] = None
            row['page_error'] = str(error)
        row['decision'], row['reason'] = classify(row, datetime.now(timezone.utc).year, args.review_age)
    selection = dict(categories=args.categories, review_age=args.review_age, prepared_at=now(),
                     complete=not errors, errors=errors, books=list(rows.values()))
    write_json(selection_path, selection)
    review_path = directory / 'review.json'
    old_reviews = {row['entry_id']: row for row in read_json(review_path)} if review_path.exists() else {}
    write_json(review_path, [dict(entry_id=row['entry_id'], title=row['title'],
        authors=row['authors'], year=row['year'], faves=row['faves'], entry_url=row['entry_url'],
        reason=row['reason'], decision=old_reviews.get(row['entry_id'], {}).get('decision'),
        note=old_reviews.get(row['entry_id'], {}).get('note', '')) for row in rows.values() if row['decision'] == 'review'])
    write_review_csv(directory, list(rows.values()))
    print(json.dumps(dict(counts=Counter(row['decision'] for row in rows.values()), errors=errors), indent=2))
    if errors:
        raise ValueError('Selection is incomplete; rerun prepare to retry failed feed pages')


def approved_books(selection, reviews):
    if not selection['complete']:
        raise ValueError('Cannot acquire an incomplete category selection')
    by_id = {}
    for review in reviews:
        identity = review['entry_id']
        if identity in by_id or review.get('decision') not in (None, 'include', 'skip'):
            raise ValueError('Invalid or duplicate review decision: ' + identity)
        by_id[identity] = review.get('decision')
    known = {row['entry_id'] for row in selection['books'] if row['decision'] == 'review'}
    if set(by_id) - known:
        raise ValueError('Review contains books outside this selection')
    for row in selection['books']:
        if has_gutenberg_origin(row):
            continue
        include = row['decision'] == 'include' or (row['decision'] == 'review' and by_id.get(row['entry_id']) == 'include')
        if include and row.get('faves') is not None and row['faves'] >= 1 and row['acquisitions']:
            yield row


def validate_book(path, format):
    if format == 'epub':
        with zipfile.ZipFile(path) as archive:
            if archive.read('mimetype').strip() != b'application/epub+zip' or 'META-INF/container.xml' not in archive.namelist():
                raise ValueError('Download is not an EPUB')
            if archive.testzip() is not None:
                raise ValueError('Corrupt EPUB archive')
    else:
        with path.open('rb') as stream:
            if b'%PDF-' not in stream.read(1024):
                raise ValueError('Download is not a PDF')


def epub_gutenberg_origin(path):
    with zipfile.ZipFile(path) as archive:
        container = ET.fromstring(archive.read('META-INF/container.xml'))
        rootfile = container.find('.//{urn:oasis:names:tc:opendocument:xmlns:container}rootfile')
        package = ET.fromstring(archive.read(rootfile.attrib['full-path']))
        metadata = package.find('{http://www.idpf.org/2007/opf}metadata')
        if metadata is None:
            return False
        for node in metadata:
            if node.tag.rsplit('}', 1)[-1] not in ('publisher', 'identifier', 'source'):
                continue
            value = ''.join(node.itertext()).strip()
            if gutenberg_url(value) or re.search(r'\b(?:Project Gutenberg|GITenberg)\b', value, re.I) or re.match(r'^(?:urn:)?(?:gtbg|gutenberg):', value, re.I):
                return True
    return False


def acquisition_url(url, directory, fetcher):
    if gutenberg_url(url):
        raise ExcludedOrigin('Project Gutenberg origin excluded; download stopped before reading book bytes')
    return url


def source_metadata(row):
    metadata = dict(title=row['title'], description=row['description'], year=row['year'], subjects=[
        dict(name=name, source='catalog:unglue.it') for name in row['subjects']], identifiers=[])
    for item in row['website_metadata']['identifiers']:
        if item['canonical']:
            identifier = dict(value=item['canonical'], scheme=item['scheme'],
                              scope='book' if item['scheme'] == 'isbn' else item['scope'])
            if identifier not in metadata['identifiers']:
                metadata['identifiers'].append(identifier)
    if row['languages'] and all(re.fullmatch(r'[a-zA-Z]{2,3}(?:-[a-zA-Z0-9]{2,8})*', language) for language in row['languages']):
        # Unglue.it labels the Chinese Pro Git translation with the country code
        # "cn". Preserve that declaration in evidence and use BCP-47 in records.
        metadata['languages'] = ['zh' if language == 'cn' else language for language in row['languages']]
    return metadata


def pdf_link_entry(row, acquisition):
    metadata = source_metadata(row)
    publishers = sorted({publisher for edition in row['website_metadata']['editions']
                         for publisher in edition['fields'].get('publisher', [])})
    record = dict(title=row['title'], subtitle=None, description=metadata['description'],
        contributors=[dict(contributor_id=str(uuid.uuid5(uuid.NAMESPACE_URL, row['entry_url'] + '#author:' + name)),
                           name=name, role=[97, 117, 116]) for name in row['authors']],
        book=dict(identifiers=metadata['identifiers'], publishers=publishers,
                  languages=metadata.get('languages', []), year=metadata['year'], subjects=metadata['subjects']))
    return dict(source='unglue.it', entry_id=row['entry_id'], entry_url=row['entry_url'],
        license=acquisition['rights'] or 'source-declares-free', record=record,
        file_url=acquisition.get('receipt', {}).get('url') or acquisition['advertised_url'], format='Pdf', toc=[],
        source_evidence=dict(website_metadata=row['website_metadata'], categories=row['categories'],
                             observed_at=row['observed_at'], acquisition=acquisition))


def download(args, fetcher):
    directory = args.output
    selection = read_json(directory / 'selection.json')
    reviews = read_json(args.review or directory / 'review.json')
    progress_path = directory / 'downloads.json'
    progress = read_json(progress_path) if progress_path.exists() else {}
    manifests, links, errors = [], [], []
    for row in approved_books(selection, reviews):
        try:
            candidates = sorted(row['acquisitions'], key=lambda item: (item['format'] != 'epub',
                not item.get('rights') or item['rights'].lower() == 'none', item['url']))
            previous = progress.get(row['entry_id'], {})
            successful_url = previous.get('receipt', {}).get('requested_url', previous.get('receipt', {}).get('url'))
            if previous.get('status') == 'downloaded':
                candidates.sort(key=lambda item: item['url'] != successful_url)
            attempts = []
            for link in candidates:
                file = Path('books') / (row['entry_id'] + '.' + link['format'])
                if previous.get('status') == 'downloaded' and successful_url == link['url']:
                    file = Path(previous['file'])
                receipt_path = (directory / file).with_suffix(file.suffix + '.http.json')
                if receipt_path.exists():
                    cached = read_json(receipt_path)
                    if link['url'] not in (cached['url'], cached.get('requested_url')):
                        suffix = hashlib.sha256(link['url'].encode()).hexdigest()[:12]
                        file = Path('books') / (row['entry_id'] + '-' + suffix + '.' + link['format'])
                try:
                    resolver = lambda url: acquisition_url(url, directory, fetcher)
                    receipt = fetcher.fetch(link['url'], directory / file, resolver)
                    acquisition_url(receipt['response_url'], directory, fetcher)
                    validate_book(directory / file, link['format'])
                    if link['format'] == 'epub' and epub_gutenberg_origin(directory / file):
                        raise ExcludedOrigin('Project Gutenberg origin excluded by EPUB metadata')
                    break
                except ExcludedOrigin:
                    row['gutenberg_origin'] = True
                    row['decision'], row['reason'] = 'skip', 'Project Gutenberg origin excluded'
                    raise
                except Exception as error:
                    attempts.append(dict(url=link['url'], error=str(error)))
            else:
                raise ValueError('All advertised download options failed: ' + json.dumps(attempts))
            progress[row['entry_id']] = dict(status='downloaded', file=str(file), receipt=receipt,
                                             rights=link['rights'], faves=row['faves'], failed_options=attempts, review=next(
                                                 (r for r in reviews if r['entry_id'] == row['entry_id']), None))
            for key in ('alternate_source', 'prior_failure'):
                if key in previous and successful_url == link['url']:
                    progress[row['entry_id']][key] = previous[key]
            if link['format'] == 'epub':
                metadata = source_metadata(row)
                website = row['website_metadata']
                manifests.append(dict(file=str(file), source='unglue.it', entry_id=row['entry_id'],
                                      entry_url=row['entry_url'], file_url=receipt['url'], metadata=metadata,
                                      source_evidence=dict(website_metadata=website, categories=row['categories'],
                                          observed_at=row['observed_at'], acquisition=progress[row['entry_id']])))
            else:
                links.append(pdf_link_entry(row, progress[row['entry_id']]))
        except ExcludedOrigin as error:
            progress[row['entry_id']] = dict(status='excluded', reason=str(error))
        except Exception as error:
            progress[row['entry_id']] = dict(status='failed', error=str(error))
            errors.append(dict(entry_id=row['entry_id'], error=str(error)))
        write_json(progress_path, progress)
    write_json(directory / 'import-manifest.json', manifests)
    write_json(directory / 'link-manifest.json', links)
    write_json(directory / 'selection.json', selection)
    print(json.dumps(dict(epubs_ready_for_import=len(manifests), pdf_links_ready_for_import=len(links), errors=errors), indent=2))
    if errors:
        raise ValueError('Some downloads failed; rerun download to retry')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    prepare_parser = commands.add_parser('prepare', help='Fetch category and work metadata only')
    prepare_parser.add_argument('categories', nargs='+')
    prepare_parser.add_argument('--review-age', type=int, default=5)
    download_parser = commands.add_parser('download', help='Acquire eligible and explicitly approved books')
    download_parser.add_argument('--review', type=Path)
    for command in (prepare_parser, download_parser):
        command.add_argument('--output', type=Path, required=True)
        command.add_argument('--delay', type=float, default=1.0)
    args = parser.parse_args()
    if args.delay < 0 or (args.command == 'prepare' and args.review_age < 1):
        parser.error('Delay must be nonnegative and review age must be positive')
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / '.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if args.command == 'prepare':
            prepare(args, Fetcher(args.delay))
        else:
            download(args, Fetcher(args.delay))


if __name__ == '__main__':
    main()
