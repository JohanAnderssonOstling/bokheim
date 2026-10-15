#!/usr/bin/env python3
"""Identify embedded Gutenberg placeholders and stage enriched replacements.

Audit reads curation and EPUBs without changing them. Probe saves lookup
answers, downloaded originals and a retain-covers manifest beside the audit.
Use --openlibrary for strict title/author matching first, then a looser fallback
that checks author identity, language and volume. Search results identify works;
their edition listings supply additional covers, including works without cover_i.
Up to five matched works and 300 editions per work are inspected. Multiple images
are staged for review; --exclude-cover-id skips previously rejected artwork.
All staged covers still require review before retention.
Use --google-books for Google Books, with BOKHEIM_GOOGLE_BOOKS_API_KEY set.
Install only reviewed images with `catalog DIR retain-covers MANIFEST_JSON`.
Requires Pillow for image validation and visual comparison sheets.
"""
import argparse
import collections
import hashlib
import io
import json
import os
import posixpath
import re
import sqlite3
import time
import unicodedata
import urllib.parse
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageStat


def save_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')


def embedded_cover(path):
    with zipfile.ZipFile(path) as archive:
        container = ET.fromstring(archive.read('META-INF/container.xml'))
        package = next(node.attrib['full-path'] for node in container.iter()
                       if node.tag.rsplit('}', 1)[-1] == 'rootfile')
        document = ET.fromstring(archive.read(package))
        cover_id = next((node.get('content') for node in document.iter()
                         if node.get('name') == 'cover'), None)
        items = [node for node in document.iter()
                 if node.tag.rsplit('}', 1)[-1] == 'item']
        cover = next((node for node in items
                      if 'cover-image' in node.get('properties', '').split()), None)
        if cover is None:
            cover = next((node for node in items if node.get('id') == cover_id), None)
        if cover is None:
            return None, None
        member = posixpath.normpath(posixpath.join(posixpath.dirname(package),
                                                  urllib.parse.unquote(cover.attrib['href'])))
        with Image.open(io.BytesIO(archive.read(member))) as image:
            return member, list(image.size)


def is_placeholder(member, size, entry_id):
    return bool(member and re.fullmatch(
        r'(?:\d+_)?' + re.escape(entry_id) + r'-cover\.png',
        posixpath.basename(member)) and size == [1600, 2400])


def displayed_placeholder(catalog, key, member):
    """Compare displayed artwork with the known generated EPUB cover.

    Reimports may have lost retained-cover evidence. Comparing the actual
    image prevents replacing a genuine cover merely because the EPUB still
    contains Gutenberg's generated artwork. Small JPEG differences are normal.
    """
    current = catalog / 'covers' / (key + '-card.jpg')
    if not current.is_file():
        return True
    with zipfile.ZipFile(catalog / 'books' / (key + '.epub')) as archive, \
            Image.open(io.BytesIO(archive.read(member))) as generated, \
            Image.open(current) as displayed:
        generated = generated.convert('RGB').resize((32, 48))
        displayed = displayed.convert('RGB').resize((32, 48))
        difference = ImageStat.Stat(ImageChops.difference(generated, displayed))
        return sum(difference.mean) / 3 <= 5


def audit(catalog, output):
    rows = []
    # URI mode=ro also prevents accidentally creating a new curation database.
    with sqlite3.connect((catalog / 'curation.sqlite3').resolve().as_uri() + '?mode=ro', uri=True) as db:
        query = """SELECT b.hash,b.title,b.author,b.record,s.entry_id,s.entry_url,
                   e.metadata FROM book b JOIN book_source s USING(hash)
                   LEFT JOIN source_entry e ON e.source=s.source AND e.entry_id=s.entry_id
                   WHERE s.source='gutenberg.org' ORDER BY b.rowid"""
        records = db.execute(query).fetchall()
    # Release the SQLite read lock before scanning potentially thousands of EPUBs.
    for key, title, author, record, entry_id, page, evidence in records:
        row = dict(hash=key, title=title, author=author, entry_id=entry_id,
                   entry_url=page, record=json.loads(record))
        try:
            member, size = embedded_cover(catalog / 'books' / (key + '.epub'))
            # Gutenberg generated art is ID-cover.png, optionally preceded
            # by the EPUB builder's numeric prefix. Ordinary cover.png,
            # jacket scans and arbitrary covers do not match this rule.
            generated = is_placeholder(member, size, entry_id)
            replacement = json.loads(evidence or '{}').get('cover')
            row.update(embedded_cover=member, embedded_size=size,
                       classification='generated' if generated else 'original_or_unknown',
                       displayed_placeholder=(displayed_placeholder(catalog, key, member)
                                              if generated else False),
                       retained_cover_evidence=replacement,
                       has_card=(catalog / 'covers' / (key + '-card.jpg')).is_file(),
                       has_detail=(catalog / 'covers' / (key + '-detail.jpg')).is_file())
        except Exception as error:
            row.update(classification='error', error=str(error))
        rows.append(row)
    output.mkdir(parents=True, exist_ok=True)
    save_json(output / 'audit.json', rows)
    summary = dict(total=len(rows), classifications=dict(collections.Counter(
        row['classification'] for row in rows)),
        already_retained=sum(bool(row.get('retained_cover_evidence')) for row in rows))
    summary['displayed_placeholders'] = sum(row.get('displayed_placeholder') is True for row in rows)
    save_json(output / 'summary.json', summary)
    print(json.dumps(summary))


def request(url, body=None):
    headers = {'User-Agent': 'bokheim-catalog-cover-audit'}
    if body is not None:
        body = json.dumps(body).encode()
        headers['Content-Type'] = 'application/json'
    with urllib.request.urlopen(urllib.request.Request(url, data=body, headers=headers), timeout=90) as response:
        data = response.read(8 * 1024 * 1024 + 1)
        if len(data) > 8 * 1024 * 1024:
            raise ValueError('response exceeds 8 MiB')
        return data


def match_key(value):
    value = ''.join(c for c in unicodedata.normalize('NFKD', value.casefold())
                    if not unicodedata.combining(c))
    return ' '.join(''.join(c if c.isalnum() else ' ' for c in value).split())


def google_image_host(host):
    return (host in ('books.google.com', 'books.google.se', 'books.googleusercontent.com')
            or (host or '').endswith('.googleusercontent.com'))


def author_name_match_key(value):
    """Mirror shared author keys: honorifics, life-date years and duplicates."""
    honorifics = {'dr', 'doctor', 'prof', 'professor', 'mr', 'mrs', 'ms',
                 'miss', 'sir', 'dame', 'rev', 'reverend', 'baron'}
    words = []
    def is_year(word):
        return 3 <= len(word) <= 4 and word.isascii() and word.isdigit()
    for part in value.split(','):
        raw = part.split()
        def date_label(i, word):
            following = match_key(raw[i + 1]).split() if i + 1 < len(raw) else []
            return (word.lstrip('(').rstrip('.') in {'b', 'born', 'd', 'died', 'fl'}
                    and following and is_year(following[0]))
        text = ' '.join(word for i, word in enumerate(raw) if not date_label(i, word))
        tokens = [word for word in match_key(text).split() if not is_year(word)]
        while tokens and tokens[0] in honorifics:
            tokens.pop(0)
        for word in tokens:
            if not words or words[-1] != word:
                words.append(word)
    return ' '.join(words)


def author_words_compatible(left, right):
    if left and sorted(left) == sorted(right):
        return True
    if (len(left) < 2 or len(right) < 2
            or not any(len(word) > 1 and word in right for word in left)):
        return False
    left, right = list(left), list(right)
    for word in list(left):
        if word in right:
            left.remove(word)
            right.remove(word)
    short, long = (left, right) if len(left) <= len(right) else (right, left)
    edges = [[i for i, b in enumerate(long)
              if (len(a) == 1 and b.startswith(a)) or (len(b) == 1 and a.startswith(b))]
             for a in short]
    owners = [None] * len(long)
    def assign(i, seen):
        for j in edges[i]:
            if j in seen:
                continue
            seen.add(j)
            if owners[j] is None or assign(owners[j], seen):
                owners[j] = i
                return True
        return False
    return (all(assign(i, set()) for i in range(len(short)))
            and all(owner is not None or len(word) == 1 for word, owner in zip(long, owners)))


def author_matches(local, remote, strict=False):
    left, right = author_name_match_key(local), author_name_match_key(remote)
    if left and left == right:
        return True
    if strict:
        return False
    # Known spellings of classical authors, rather than guessing translations.
    aliases = {'πλατων': 'plato', 'αριστοτελης': 'aristotle',
               'epiktetos': 'epictetus', 'επικτητος': 'epictetus'}
    left, right = aliases.get(left, left), aliases.get(right, right)
    if left and left == right:
        return True

    def parts(name):
        words = author_name_match_key(name).split()
        suffixes = {'jr': 'jr', 'junior': 'jr', 'sr': 'sr', 'senior': 'sr',
                    'ii': 'ii', 'iii': 'iii', 'iv': 'iv', 'v': 'v'}
        suffix = suffixes.get(words[-1]) if words else None
        return (words[:-1] if suffix else words), suffix

    (a, a_suffix), (b, b_suffix) = parts(local), parts(remote)
    if a_suffix and b_suffix and a_suffix != b_suffix:
        return False
    return author_words_compatible(a, b)


def title_volume(title):
    match = re.search(r'\b(?:volume|vol\.?|part|book)\s+([ivxlcdm]+|\d+)\b', title, re.I)
    if not match:
        return None
    value = match.group(1).lower()
    if value.isdigit():
        return int(value)
    total, previous = 0, 0
    for letter in reversed(value):
        number = {'i': 1, 'v': 5, 'x': 10, 'l': 50, 'c': 100, 'd': 500, 'm': 1000}[letter]
        total += -number if number < previous else number
        previous = max(previous, number)
    return total


_TITLE_NUMBER = r'\b(?:\d+|[ivxlcdm]+)\b'
_TITLE_PART = re.compile(
    r'\b(volumes?|vols?\.?|parts?|books?|essays?)\s*\.?\s*('
    + _TITLE_NUMBER + r'(?:\s*\.?\s*(?:and|&|,|[-–—])\s*'
    + _TITLE_NUMBER + r')*)', re.I)
_TITLE_CONTENT = re.compile(
    r'\b(?:other\s+(?:essays|poems|stories|works|dialogues)|selected|collected|'
    r'complete\s+works|extracts|selections|abridged|abridgment|abridgement|'
    r'adapted|adaptation|retold|retelling)\b', re.I)


def title_part_numbers(value):
    numbers = [title_volume('Volume ' + n) for n in re.findall(_TITLE_NUMBER, value, re.I)]
    if re.search(r'[-–—]', value) and len(numbers) == 2:
        first, last = numbers
        if first <= last and last - first <= 100:
            return tuple(range(first, last + 1))
    return tuple(sorted(numbers))


def title_structure(value):
    parts = set()
    for match in _TITLE_PART.finditer(value):
        label = match[1].lower().rstrip('.')
        kind = ('volume' if label.startswith('vol') else
                'part' if label.startswith('part') else
                'book' if label.startswith('book') else 'essay')
        parts.add((kind, title_part_numbers(match[2])))
    return parts


def clean_title_notes(value):
    # The series total is not the selected volume: keep 2 in Vol. 2 of 3.
    value = re.sub(r'\(\s*of\s+\d+\s*\)', '', value, flags=re.I)
    value = re.sub(r'(' + _TITLE_PART.pattern + r')\s+of\s+\d+\b',
                   lambda match: match[1], value, flags=re.I)
    value = re.sub(r'\(\s*(?:\d{4}|annotated|illustrated|unabridged|[^)]*edition)\s*\)',
                   '', value, flags=re.I)
    return value


def loose_title_key(value):
    value = clean_title_notes(value)
    def part_key(match):
        label = match[1].lower().rstrip('.')
        kind = ('volume' if label.startswith('vol') else
                'part' if label.startswith('part') else
                'book' if label.startswith('book') else 'essay')
        return ' ' + kind + ' ' + ' '.join(map(str, title_part_numbers(match[2]))) + ' '
    key = match_key(_TITLE_PART.sub(part_key, value))
    key = re.sub(r'^(?:the|a|an)\s+', '', key)
    return re.sub(r'\bor\s+(?:the|a|an)\s+', 'or ', key)


def title_main(value):
    value = clean_title_notes(value)
    value = re.split(r'\s*[:;]\s*|\s+[—–/-]\s+|\s+[\[(]', value, maxsplit=1)[0]
    return re.sub(r'^(?:the|a|an)\s+', '', match_key(_TITLE_PART.sub('', value)))


def loose_title_matches(row, found):
    own = row['title']
    subtitle = (row.get('record') or {}).get('subtitle')
    complete = own + (': ' + subtitle if subtitle else '')
    if title_structure(complete) != title_structure(found):
        return False
    if {match_key(m[0]) for m in _TITLE_CONTENT.finditer(complete)} != {
            match_key(m[0]) for m in _TITLE_CONTENT.finditer(found)}:
        return False
    remote = loose_title_key(found)
    local_keys = {loose_title_key(own), loose_title_key(complete)}
    if remote and remote in local_keys:
        return True
    # Only explicitly separated main titles may fall back to main-title lookup.
    # An arbitrary prefix must not turn a different essay/book into a match.
    local_main, remote_main = title_main(own), title_main(found)
    if not local_main or local_main != remote_main:
        return False
    local_tail = loose_title_key(_TITLE_PART.sub('', clean_title_notes(complete))) != local_main
    remote_tail = loose_title_key(_TITLE_PART.sub('', clean_title_notes(found))) != remote_main
    # Known subtitles on both sides must agree; main-title fallback remains
    # available when an edition omits the descriptive subtitle altogether.
    return not (local_tail and remote_tail)


def openlibrary_work_matches(row, response):
    """Match work identity even when its search result has no cover."""
    authors = [credit['name'] for credit in row['record']['contributors']
               if credit['role'] == [97, 117, 116]]
    language_keys = {'en': 'eng', 'fr': 'fre', 'de': 'ger', 'es': 'spa',
                     'it': 'ita', 'la': 'lat', 'sv': 'swe', 'nl': 'dut',
                     'pt': 'por', 'ru': 'rus', 'el': 'gre'}
    own_languages = row['record'].get('book', {}).get('languages', [])
    wanted = {language_keys.get(code.split('-')[0], code) for code in own_languages}
    equivalents = {'fra': 'fre', 'deu': 'ger', 'ell': 'gre', 'nld': 'dut'}
    strict, loose = [], []
    for doc in response.get('docs', []):
        languages = {equivalents.get(code, code) for code in (doc.get('language') or [])}
        if wanted and languages and not wanted.intersection(languages):
            continue
        remote_authors = doc.get('author_name') or []
        tier = None
        if (match_key(doc.get('title', '')) == match_key(row['title'])
                and any(author_matches(a, b, strict=True) for a in authors for b in remote_authors)):
            tier = 'strict'
        elif (loose_title_matches(row, doc.get('title', ''))
              and any(author_matches(a, b) for a in authors for b in remote_authors)):
            tier = 'loose'
        if tier:
            (strict if tier == 'strict' else loose).append((doc, tier))
    return strict + loose


def valid_cover_id(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def openlibrary_candidates(row, response):
    """Return search-result covers with every strict match first."""
    seen = set()
    results = []
    for doc, tier in openlibrary_work_matches(row, response):
        local_title = row['title'] + (': ' + row['record']['subtitle']
                                      if row['record'].get('subtitle') else '')
        if title_volume(local_title) != title_volume(doc.get('title', '')):
            continue
        if valid_cover_id(doc.get('cover_i')) and doc['cover_i'] not in seen:
            seen.add(doc['cover_i'])
            results.append((doc, tier))
    return results


def openlibrary_edition_candidates(row, work, evidence, max_pages=3):
    """Discover covers omitted by search, validating each edition's identity."""
    work_key = work.get('key', '')
    if not re.fullmatch(r'/works/OL\d+W', work_key):
        return []
    editions = []
    seen = set()
    for page in range(max_pages):
        url = 'https://openlibrary.org' + work_key + '/editions.json?' + urllib.parse.urlencode(
            dict(limit=100, offset=page * 100))
        try:
            answer = json.loads(request(url))
        except (OSError, ValueError) as error:
            if isinstance(error, urllib.error.HTTPError):
                error.close()
            evidence.append(dict(url=url, error=str(error)))
            break
        entries = answer.get('entries', [])
        evidence.append(dict(url=url, response=answer))
        for edition in entries:
            key = edition.get('key', '')
            if key in seen or not re.fullmatch(r'/books/OL\d+M', key):
                continue
            seen.add(key)
            # The endpoint establishes membership. If explicit work links are
            # present, reject conflicting links rather than inheriting authors.
            works = edition.get('works') or []
            if works and work_key not in [value.get('key') for value in works]:
                continue
            author_keys = set(work.get('author_key') or [])
            edition_authors = {value.get('key', '').rsplit('/', 1)[-1]
                               for value in (edition.get('authors') or [])}
            if author_keys and edition_authors and not author_keys.intersection(edition_authors):
                continue
            title = edition.get('title', '')
            if edition.get('subtitle'):
                title += ': ' + edition['subtitle']
            if edition.get('volume'):
                title += ', Volume ' + str(edition['volume'])
            local_title = row['title']
            if row['record'].get('subtitle'):
                local_title += ': ' + row['record']['subtitle']
            if title_volume(local_title) != title_volume(title):
                continue
            # Editions inherit the author identity of the already matched work.
            # Language and volume are checked on this edition, not its work.
            doc = dict(work, title=title, cover_edition_key=key,
                       language=[value['key'].rsplit('/', 1)[-1]
                                 for value in (edition.get('languages') or []) if value.get('key')])
            for cover_id in edition.get('covers') or []:
                if not valid_cover_id(cover_id):
                    continue
                candidate = dict(doc, cover_i=cover_id, edition=edition)
                matches = openlibrary_candidates(dict(row, title=local_title), dict(docs=[candidate]))
                if matches:
                    editions.extend(matches)
        if not entries or page * 100 + len(entries) >= answer.get('size', len(entries)):
            break
    # Prefer exact edition titles before subtitle/name variants.
    return [item for item in editions if item[1] == 'strict'] + [
        item for item in editions if item[1] == 'loose']


def openlibrary_cover_candidates(row, response, evidence, max_works=5, max_pages=3):
    """Discover edition covers, keeping strict identity ahead of looser matches."""
    matches = openlibrary_work_matches(row, response)
    candidates = openlibrary_candidates(row, response)
    seen_works = set()
    for tier in ('strict', 'loose'):
        group = [(doc, match) for doc, match in matches if match == tier]
        for work, _ in group:
            key = work.get('key')
            if (not isinstance(key, str) or not re.fullmatch(r'/works/OL\d+W', key)
                    or key in seen_works or len(seen_works) >= max_works):
                continue
            seen_works.add(key)
            candidates.extend(openlibrary_edition_candidates(row, work, evidence, max_pages))

    def priority(item):
        doc, tier = item
        edition = doc.get('edition') or {}
        publishers = ' '.join(edition.get('publishers') or []).lower()
        # Established reprint publishers often have designed jackets rather
        # than scanned title pages. This only ranks already verified matches;
        # the artwork still needs visual review.
        jacket = any(name in publishers for name in (
            'dover', 'penguin', 'oxford', 'cambridge', 'vintage', 'norton',
            'routledge', 'princeton', 'macmillan', 'modern library', 'free press'))
        return (tier != 'strict', not jacket, not bool(edition))

    seen_covers = set()
    for doc, tier in sorted(candidates, key=priority):
        if doc['cover_i'] not in seen_covers:
            seen_covers.add(doc['cover_i'])
            yield doc, tier


def openlibrary_lookup(row):
    authors = [credit['name'] for credit in row['record']['contributors']
               if credit['role'] == [97, 117, 116]]
    if not authors:
        return [], None
    params = urllib.parse.urlencode(dict(
        title=row['title'], author=authors[0], limit=100,
        fields='key,title,author_name,author_key,cover_i,cover_edition_key,language'))
    url = 'https://openlibrary.org/search.json?' + params
    answer = json.loads(request(url))
    evidence = dict(search_url=url, response=answer, edition_lookups=[])
    return openlibrary_cover_candidates(row, answer, evidence['edition_lookups']), evidence


def openlibrary_cover(row):
    candidates, evidence = openlibrary_lookup(row)
    return next((doc for doc, _ in candidates), None), evidence


def google_matches(row, info, strict=True):
    own = row['record']
    title = own['title'] + (': ' + own['subtitle'] if own.get('subtitle') else '')
    found = info.get('title', '') + (': ' + info['subtitle'] if info.get('subtitle') else '')
    authors = [credit['name'] for credit in own['contributors'] if credit['role'] == [97, 117, 116]]
    languages = own['book'].get('languages', [])
    title_match = (match_key(title) == match_key(found) if strict
                   else loose_title_matches(dict(title=title), found))
    return (title_match
            and any(author_matches(local, remote, strict=strict)
                    for local in authors for remote in info.get('authors', []))
            and (not languages or not info.get('language')
                 or info['language'] in [language.split('-')[0] for language in languages]))


def google_request(url):
    # Books search can intermittently return backendFailed for valid keys.
    # Retry those service errors, but stop immediately on daily quota failures.
    for attempt in range(4):
        try:
            return request(url)
        except urllib.error.HTTPError as error:
            error.close()
            if error.code not in (500, 502, 503, 504) or attempt == 3:
                raise
            time.sleep(2 ** attempt)


def google_cover(row, key):
    authors = [credit['name'] for credit in row['record']['contributors'] if credit['role'] == [97, 117, 116]]
    if not authors:
        return [], None
    # Plain title/author searches currently return editions where qualified
    # intitle/inauthor searches can return empty results. Identity is checked
    # against the returned metadata below, independently of search ranking.
    params = dict(q=row['title'] + ' ' + authors[0],
                  maxResults=20, printType='books')
    public_url = 'https://www.googleapis.com/books/v1/volumes?' + urllib.parse.urlencode(params)
    # Persist the search receipt without its project key.
    answer = json.loads(google_request(public_url + '&' + urllib.parse.urlencode(dict(key=key))))
    strict_candidates, loose_candidates = [], []
    for volume in answer.get('items', []):
        info = volume.get('volumeInfo', {})
        if google_matches(row, info):
            candidates, tier = strict_candidates, 'strict'
        elif google_matches(row, info, strict=False):
            candidates, tier = loose_candidates, 'loose'
        else:
            continue
        for name in ('extraLarge', 'large', 'medium', 'small', 'thumbnail', 'smallThumbnail'):
            link = info.get('imageLinks', {}).get(name)
            if not link:
                continue
            url = urllib.parse.urlsplit(link)
            trusted = google_image_host(url.hostname)
            if not trusted or url.username or url.password or url.scheme not in ('http', 'https'):
                continue
            query = dict(urllib.parse.parse_qsl(url.query))
            query['w'] = '1280'
            # Preserve zoom and printsec: changing them can fetch a text page.
            image = urllib.parse.urlunsplit(('https', url.netloc, url.path, urllib.parse.urlencode(query), ''))
            if not any(candidate['image_url'] == image for candidate in candidates):
                candidates.append(dict(image_url=image, volume_id=volume['id'], volume_info=info,
                                       match_tier=tier))
    candidates, seen = [], set()
    for candidate in strict_candidates + loose_candidates:
        if candidate['image_url'] not in seen:
            seen.add(candidate['image_url'])
            candidates.append(candidate)
    return candidates, dict(search_url=public_url, response=answer)


def comparison(catalog, output, replacements):
    for start in range(0, len(replacements), 12):
        batch = replacements[start:start + 12]
        sheet = Image.new('RGB', (960, ((len(batch) + 2) // 3) * 265), '#eeeeee')
        draw = ImageDraw.Draw(sheet)
        for index, row in enumerate(batch):
            x, y = index % 3 * 320, index // 3 * 265
            paths = [catalog / 'covers' / (row['hash'] + '-card.jpg'),
                     output / row['file_path']]
            for offset, path in zip((0, 160), paths):
                if path.is_file():
                    with Image.open(path) as image:
                        image.thumbnail((150, 220))
                        sheet.paste(image.convert('RGB'), (x + offset, y))
            draw.text((x, y + 222), row['title'][:42], fill='black')
            draw.text((x, y + 240), 'Gutenberg             Enriched', fill='black')
        sheet.save(output / ('comparison-%03d.jpg' % (start // 12 + 1)))


def probe(catalog, output, origin, limit, openlibrary=False, google_books=False,
          cover_candidates=4, exclude_cover_ids=()):
    rows = json.loads((output / 'audit.json').read_text())
    def eligible(row):
        if row['classification'] != 'generated' or row.get('displayed_placeholder') is False:
            return False
        if row.get('displayed_placeholder') is True:
            return True
        retained = row.get('retained_cover_evidence') or {}
        if google_books:
            return not google_image_host(urllib.parse.urlsplit(retained.get('source_url', '')).hostname)
        return not retained
    selected = [row for row in rows if eligible(row)][:limit]
    google_key = os.environ.get('BOKHEIM_GOOGLE_BOOKS_API_KEY', '').strip()
    if google_books and not google_key:
        raise ValueError('Google Books bulk lookup needs BOKHEIM_GOOGLE_BOOKS_API_KEY; reviewed cached images can still be retained')
    origin = origin.rstrip('/') + '/'
    answers, replacements, manifest, previews = [], [], [], []
    (output / 'images').mkdir(exist_ok=True)
    for row in selected:
        result = dict(hash=row['hash'], entry_id=row['entry_id'], title=row['title'])
        try:
            url = None
            candidate_data = None
            if google_books:
                candidates, answer = google_cover(row, google_key)
                result['google_books'] = answer
                for candidate in candidates:
                    time.sleep(1)
                    try:
                        candidate_data = request(candidate['image_url'])
                        with Image.open(io.BytesIO(candidate_data)) as image:
                            image.load()
                            if image.width < 600 or image.height < 600:
                                continue
                        url = candidate['image_url']
                        result['matched_volume'] = candidate
                        result['match_tier'] = candidate['match_tier']
                        break
                    except (OSError, ValueError):
                        continue
                time.sleep(1)
            elif openlibrary:
                candidates, answer = openlibrary_lookup(row)
                result['openlibrary'] = answer
                result['candidate_attempts'] = []
                result['cover_candidates'] = []
                for match, tier in candidates:
                    if match['cover_i'] in exclude_cover_ids:
                        continue
                    candidate_url = 'https://covers.openlibrary.org/b/id/%s-L.jpg?default=false' % match['cover_i']
                    attempt = dict(image_url=candidate_url, match_tier=tier, matched_work=match)
                    result['candidate_attempts'].append(attempt)
                    time.sleep(1)
                    try:
                        data = request(candidate_url)
                        with Image.open(io.BytesIO(data)) as image:
                            image.load()
                            if image.width < 200 or image.height < 300:
                                raise ValueError('replacement is too small: %sx%s' % image.size)
                        candidate_path = 'images/' + row['hash'] + '-' + str(match['cover_i']) + '.image'
                        (output / candidate_path).write_bytes(data)
                        staged = dict(image_url=candidate_url, file_path=candidate_path,
                                      matched_work=match, match_tier=tier,
                                      image_size=list(image.size), image_sha256=hashlib.sha256(data).hexdigest())
                        result['cover_candidates'].append(staged)
                        previews.append(dict(row, **staged))
                        if url is None:
                            url, candidate_data = candidate_url, data
                            result.update(matched_work=match, match_tier=tier)
                        if len(result['cover_candidates']) >= cover_candidates:
                            break
                    except (OSError, ValueError) as error:
                        if isinstance(error, urllib.error.HTTPError):
                            error.close()
                        attempt['error'] = str(error)
                if result['candidate_attempts'] and not url:
                    result['error'] = 'All matching Open Library covers failed validation or download'
            else:
                facts = dict(key=row['hash'], record=row['record'], audiobook=False,
                             needs_cover=True, readings=[], recording=None)
                answer = json.loads(request(urllib.parse.urljoin(origin, 'v3/books'), {'books': [facts]}))
                result['response'] = answer
                results = answer['results']
                if len(results) != 1 or results[0]['key'] != row['hash']:
                    raise ValueError('metadata response has unexpected book identity')
                match = results[0]
                if match['status'] == 'found' and match.get('cover'):
                    cover_id = match['cover']['id']
                    url = urllib.parse.urljoin(origin, 'v3/covers/' + urllib.parse.quote(cover_id, safe=''))
            if url:
                data = candidate_data if candidate_data is not None else request(url)
                with Image.open(io.BytesIO(data)) as image:
                    image.load()
                    width, height = image.size
                    if width < 200 or height < 300:
                        raise ValueError('replacement is too small: %sx%s' % (width, height))
                image_path = 'images/' + row['hash'] + '.image'
                (output / image_path).write_bytes(data)
                result.update(image_url=url, file_path=image_path, image_size=[width, height],
                              image_sha256=hashlib.sha256(data).hexdigest())
                replacements.append(dict(row, **{key: result[key] for key in
                    ('image_url', 'file_path', 'image_size', 'image_sha256')}))
                manifest.append(dict(source='gutenberg.org', entry_id=row['entry_id'],
                                     image_url=url, file_path=image_path))
        except Exception as error:
            result['error'] = str(error)
            if google_books and getattr(error, 'code', None) in (403, 429):
                answers.append(result)
                save_json(output / 'probe.json', answers)
                save_json(output / 'replacements.json', replacements)
                save_json(output / 'retain-covers.json', manifest)
                comparison(catalog, output, replacements)
                raise ValueError('Google Books denied the bulk lookup or exhausted its quota; completed candidates are retained') from None
        answers.append(result)
        # Persist each result so an interrupted trial remains inspectable.
        save_json(output / 'probe.json', answers)
        save_json(output / 'replacements.json', replacements)
        save_json(output / 'retain-covers.json', manifest)
        print(json.dumps(dict(entry_id=row['entry_id'], title=row['title'],
                              replacement='file_path' in result, error=result.get('error'))), flush=True)
    comparison(catalog, output, previews if openlibrary else replacements)
    print(json.dumps(dict(probed=len(answers), replacements=len(replacements))))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('audit', 'probe'))
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--metadata-url', default='https://meta.bokheim.se/')
    parser.add_argument('--limit', type=int, default=25)
    parser.add_argument('--cover-candidates', type=int, default=4,
                        help='Open Library images to stage per book for visual review (default: 4)')
    parser.add_argument('--exclude-cover-id', type=int, action='append', default=[],
                        help='skip an Open Library cover already rejected during review; repeat as needed')
    provider = parser.add_mutually_exclusive_group()
    provider.add_argument('--openlibrary', action='store_true',
                        help='try strict Open Library title/author matches, then looser matches and alternate covers')
    provider.add_argument('--google-books', action='store_true',
                          help='stage Google Books covers at least 600 pixels wide; uses BOKHEIM_GOOGLE_BOOKS_API_KEY')
    args = parser.parse_args()
    if args.limit < 1 or args.cover_candidates < 1:
        parser.error('--limit and --cover-candidates must be positive')
    if args.command == 'audit':
        audit(args.catalog, args.output)
    else:
        probe(args.catalog, args.output, args.metadata_url, args.limit, args.openlibrary, args.google_books,
              args.cover_candidates, args.exclude_cover_id)


if __name__ == '__main__':
    main()
