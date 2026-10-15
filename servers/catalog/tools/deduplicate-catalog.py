#!/usr/bin/env python3
"""Remove exact and reviewed title-alias duplicates, preferring Public Domain Library."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
from posixpath import dirname, join
import re
import sqlite3
import xml.etree.ElementTree as ET
import zipfile

PREFERRED_SOURCE = 'publicdomainlibrary.org'
# Both names appear in the preferred EPUB's package metadata. Other titles,
# authors, and separately named volumes continue to use exact matching.
REVIEWED_TITLE_ALIASES = {
    ('Adam Smith', 'An Inquiry into the Nature and Causes of the Wealth of Nations'): 'The Wealth of Nations',
}
ENTRIES = "SELECT 'book' kind,hash id,1 format,title,subtitle,author,record,toc,rowid ordinal FROM book UNION ALL SELECT 'external_book',entry_key,format,title,subtitle,author,record,toc,rowid FROM external_book"


def match_key(row):
    title = REVIEWED_TITLE_ALIASES.get((row['author'], row['title']), row['title'])
    return row['format'], title, row['author'], json.dumps(row.get('volume_scope'), sort_keys=True)


def titlepage_volume_markers(archive, package_path, package):
    """Read explicit titlepage volume labels when package titles omit them."""
    manifest = {item.get('id'): item for item in package.find('{*}manifest')}
    if 'merged-nav' in manifest:
        return []
    markers = set()
    for reference in package.find('{*}spine'):
        item = manifest.get(reference.get('idref'))
        if item is None or item.get('media-type') != 'application/xhtml+xml' or 'nav' in item.get('properties', '').split():
            continue
        member = join(dirname(package_path), item.get('href'))
        body = ET.fromstring(archive.read(member)).find('{*}body')
        if body is None:
            continue
        seen_text = 0
        for node in body.iter():
            seen_text += len(node.text or '') + len(node.tail or '')
            if seen_text > 12000:
                break
            if node.tag.rsplit('}', 1)[-1] in ('p', 'div', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6'):
                label = ' '.join(''.join(node.itertext()).split())
                if re.fullmatch(r'(?:volume|vol\.?|tome|tomo|deel)\s+(?:\d+|[ivxlcdm]+)\.?', label, re.I):
                    markers.add(label)
        if seen_text > 1000:
            break
    return sorted(markers)


def volume_scope(row, catalog=None):
    """Different source volumes can share an enriched display title."""
    title = row['title']
    titlepage_markers = []
    swedish = False
    if catalog is not None and row['kind'] == 'book':
        try:
            with zipfile.ZipFile(catalog / 'books' / (row['id'] + '.epub')) as archive:
                package = ET.fromstring(archive.read('META-INF/container.xml')).find('.//{*}rootfile').get('full-path')
                publication = ET.fromstring(archive.read(package))
                title = publication.find('{*}metadata').findtext('{*}title') or title
                swedish = publication.find('{*}metadata').findtext('{*}language', '') in ('sv', 'swe', 'sv-SE')
                # PG16643 is Turpin's school selection, whereas Emerson's
                # complete paired essay series can also display as "Essays".
                # Its original title and editor were checked against the text.
                if (title == 'Essays by Ralph Waldo Emerson'
                        and any(e.text == 'Edna Henry Lee Turpin'
                                for e in publication.find('{*}metadata').findall('{*}contributor'))):
                    return {'kind': 'selected_collection', 'title': title,
                            'editor': 'Edna Henry Lee Turpin'}
                titlepage_markers = titlepage_volume_markers(archive, package, publication)
        except (OSError, KeyError, AttributeError, ET.ParseError, zipfile.BadZipFile):
            pass
    match = re.search(r'\b(volumes?|vols?|v(?=\.)|tomes?|parts?|teil|parte|band|deel|delen|books?|buch)(?:\.?\s+|\.\s*|\s*[,;:\-–—]\s*)(\d+|[ivxlcdm]+)\b', title, re.I)
    if not match and len(titlepage_markers) == 1:
        match = re.search(r'\b(volumes?|vols?|tomes?|tomos?|deel)(?:\.?\s+|\.\s*)(\d+|[ivxlcdm]+)\b', titlepage_markers[0], re.I)
        title = titlepage_markers[0]
    if not match:
        compound = re.search(r'(?:\.\s*|\s+)([IVX]{1,5}):([1-9][0-9]?)\.?$', title) if swedish else None
        native = re.search(r'\b(?:första|andra|tredje|fjärde|fjerde|femte|sjätte|förra|senare)\s+(?:delen|bandet)\b', title, re.I)
        bare = re.search(r'(?:\.\s*|\s+)([IVX]{1,5}|[1-9]|1[0-9]|20)(?:\.?|\s+\[[^]]+\])$', title) if swedish else None
        if compound or native or bare:
            return {'kind': 'unverified_numbered_scope', 'marker': normalized((compound or native or bare)[0])}
        if len(titlepage_markers) > 1:
            return {'kind': 'unverified_titlepage_extent', 'identity': row['id']}
        position = re.search(r'\((\d+)\s*(?:/|\s+(?:of|de|von|van|av)\s+)\s*(\d+)\)', title, re.I)
        if (position and 1 <= int(position[1]) <= int(position[2])
                and not (int(position[1]) >= 1000 and int(position[2]) >= 1000)):
            return {'kind': 'volume', 'first': int(position[1]), 'end': None,
                    'connector': None, 'sections': []}
        # These are separately titled essay collections in Saunders's series,
        # not identical books sharing a generic enriched display title.
        named_essay = re.fullmatch(r'The Essays of Arthur Schopenhauer\s*[:;/]\s*(.+)', title, re.I)
        if named_essay:
            return {'kind': 'named_collection', 'title': normalized(named_essay[1])}
        return None
    values = {'i': 1, 'v': 5, 'x': 10, 'l': 50, 'c': 100, 'd': 500, 'm': 1000}

    def number(text):
        if text.isdecimal(): return int(text)
        text = text.lower()
        return sum(-values[x] if i + 1 < len(text) and values[x] < values[text[i + 1]] else values[x] for i, x in enumerate(text))

    suffix = title[match.end():]
    repeated = r'(?:(?:volumes?|vols?|v\.)\s+)?' if match[1].casefold() in ('volume', 'volumes', 'vol', 'vols', 'v') else ''
    end = re.match(r'\s*(and|to|[-–—])\s*' + repeated + r'(\d+|[ivxlcdm]+)\b', suffix, re.I)
    to_end = re.match(r'\s*to\s+the\s+end\b', suffix, re.I)
    secondary = re.findall(r'\b(slice|no|chapter|essay|part)(?:\.?\s+|\.\s*)(\d+|[ivxlcdm]+)\b', suffix, re.I)
    kind = match[1].casefold()
    return {'kind': 'book' if kind in ('book', 'books', 'buch') else ('part' if kind in ('part', 'parts', 'teil', 'parte') else 'volume'),
            'first': number(match[2]), 'end': number(end[2]) if end else None,
            'connector': end[1].casefold() if end else ('to the end' if to_end else None),
            'sections': [[kind.casefold().rstrip('.'), number(n)] for kind, n in secondary]}


def normalized(title):
    return re.sub(r'^the ', '', ' '.join(re.findall(r'\w+', title.casefold())))


def plan(db, catalog):
    sources = defaultdict(set)
    evidence = defaultdict(list)
    for key, source in db.execute('SELECT hash,source FROM book_source UNION ALL SELECT entry_key,source FROM external_book'):
        sources[key].add(source)
    for book_hash, external_key, metadata in db.execute('SELECT book_hash,external_entry_key,metadata FROM source_entry'):
        for key in (book_hash, external_key):
            if key:
                evidence[key].append(json.loads(metadata))
    columns = ['kind', 'id', 'format', 'title', 'subtitle', 'author', 'record', 'toc', 'ordinal']
    entries = [dict(zip(columns, row)) for row in db.execute(ENTRIES)]
    for row in entries:
        row['volume_scope'] = volume_scope(row, catalog)

    def score(row):
        record = json.loads(row['record'])
        details = record.get('book', {})
        languages = details.get('languages', [])
        source_title_matches = any(normalized(item.get('title', '')).startswith(normalized(row['title'])) for item in evidence[row['id']])
        nav_count = len(json.loads(row['toc'] or '[]'))
        covers = sum((catalog / 'covers' / (row['id'] + '-' + size + '.jpg')).is_file() for size in ('card', 'detail'))
        votes = max((int(item.get('rating_count') or 0) for item in evidence[row['id']]), default=0)
        readable = row['kind'] == 'book' and (catalog / 'books' / (row['id'] + '.epub')).is_file()
        language_rank = 2 if any(lang.split('-')[0] == 'en' for lang in languages) else (1 if 'sv' in languages else 0)
        return (PREFERRED_SOURCE in sources[row['id']], readable, source_title_matches, language_rank, not row['subtitle'], nav_count, covers, len(details.get('subjects', [])), bool(record.get('description')), votes, -row['ordinal'])

    groups = defaultdict(list)
    for row in entries:
        groups[match_key(row)].append(row)
    decisions = []
    for key, rows in sorted(groups.items()):
        if len(rows) < 2:
            continue
        kept = max(rows, key=score)
        if kept['kind'] == 'book' and not (catalog / 'books' / (kept['id'] + '.epub')).is_file():
            raise ValueError('Preferred retained book file is missing: ' + kept['id'])
        decisions.append(dict(format=key[0], title=key[1], author=key[2], kept=kept,
                              kept_sources=sorted(sources[kept['id']]),
                              removed=[row for row in rows if row is not kept]))
    return decisions


def apply(db, decisions, catalog=None, *, retain_entries=()):
    retain_entries = set(retain_entries)
    # Reviewed plans may contain title/author aliases. Media formats and the
    # preferred source remain mandatory boundaries even for those plans.
    for group in decisions:
        kept = group['kept']
        rows = [kept] + group['removed']
        if any((row['kind'], row['id']) in retain_entries for row in group['removed']):
            raise ValueError('A retained catalog entry cannot be removed')
        preferred = []
        for row in rows:
            if row['kind'] not in ('book', 'external_book'):
                raise ValueError('Unknown catalog entry kind')
            column = 'hash' if row['kind'] == 'book' else 'entry_key'
            format_sql = '1' if row['kind'] == 'book' else 'format'
            stored = db.execute(f"SELECT {format_sql},record FROM {row['kind']} WHERE {column}=?", (row['id'],)).fetchone()
            if not stored or stored[1] != row['record']:
                raise ValueError('Reviewed entry changed before removal: ' + row['id'])
            if stored[0] != kept['format'] or row['format'] != kept['format']:
                raise ValueError('Entries in different media formats cannot be deduplicated')
            if row.get('volume_scope') != kept.get('volume_scope'):
                raise ValueError('Different source volumes or excerpts cannot be deduplicated')
            if catalog is not None and volume_scope(row, catalog) != row.get('volume_scope'):
                raise ValueError('Reviewed source volume changed before removal')
            source_query = 'SELECT 1 FROM book_source WHERE hash=? AND source=?' if row['kind'] == 'book' else 'SELECT 1 FROM external_book WHERE entry_key=? AND source=?'
            preferred.append(bool(db.execute(source_query, (row['id'], PREFERRED_SOURCE)).fetchone()))
        if any(preferred) and not preferred[0]:
            raise ValueError('The Public Domain Library edition must be kept')
    for group in decisions:
        kept = group['kept']
        for row in group['removed']:
            if row['kind'] == 'book' and kept['kind'] == 'book':
                db.execute('INSERT OR IGNORE INTO book_source(hash,source,entry_id,entry_url,file_url) SELECT ?,source,entry_id,entry_url,file_url FROM book_source WHERE hash=?', (kept['id'], row['id']))
                db.execute('UPDATE source_entry SET book_hash=? WHERE book_hash=?', (kept['id'], row['id']))
            column = 'hash' if row['kind'] == 'book' else 'entry_key'
            if db.execute(f"DELETE FROM {row['kind']} WHERE {column}=?", (row['id'],)).rowcount != 1:
                raise ValueError('Duplicate entry changed before removal: ' + row['id'])
            for table in ('catalog_author_credit', 'catalog_author_dirty'):
                if db.execute('SELECT 1 FROM sqlite_master WHERE name=?', (table,)).fetchone():
                    db.execute(f'DELETE FROM {table} WHERE table_name=? AND record_key=?', (row['kind'], row['id']))
    if db.execute('PRAGMA integrity_check').fetchall() != [('ok',)] or db.execute('PRAGMA foreign_key_check').fetchall():
        raise ValueError('Catalog integrity verification failed')
    remaining = defaultdict(int)
    retained_counts = defaultdict(int)
    retained_present = set()
    columns = ['kind', 'id', 'format', 'title', 'subtitle', 'author', 'record', 'toc', 'ordinal']
    for values in db.execute(ENTRIES):
        row = dict(zip(columns, values))
        row['volume_scope'] = volume_scope(row, catalog)
        remaining[match_key(row)] += 1
        identity = (row['kind'], row['id'])
        if identity in retain_entries:
            retained_counts[match_key(row)] += 1
            retained_present.add(identity)
    if retained_present != retain_entries:
        raise ValueError('A retained catalog entry is missing')
    if any(remaining[match_key(group['kept'])] > max(1, retained_counts[match_key(group['kept'])]) for group in decisions):
        raise ValueError('A selected duplicate group remains after removal')


def run(args):
    catalog = args.catalog.resolve()
    path = catalog / 'curation.sqlite3'
    if args.apply and not args.output:
        raise ValueError('--apply requires a new --output directory for backups and the removal plan')
    if args.output:
        args.output.mkdir(parents=True, exist_ok=False)
    with sqlite3.connect(path.as_uri() + ('?mode=rw' if args.apply else '?mode=ro'), uri=True, timeout=45) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.execute('BEGIN IMMEDIATE' if args.apply else 'BEGIN')
        decisions = plan(db, catalog)
        receipt = dict(matching_rule='Exact format/title/displayed-author and matching source volume scope, with explicitly reviewed title aliases; subtitle and language excluded',
                       preferred_source=PREFERRED_SOURCE, groups=decisions,
                       removed_entries=sum(len(group['removed']) for group in decisions), applied=False)
        if args.output:
            (args.output / 'plan.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
        if args.apply:
            with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as reader, sqlite3.connect(args.output / 'curation-before.sqlite3') as backup:
                reader.backup(backup)
            apply(db, decisions, catalog)
            db.commit()
            receipt['applied'] = True
            (args.output / 'receipt.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
        print(json.dumps(dict(groups=len(decisions), removed_entries=receipt['removed_entries'], applied=receipt['applied'], preferred_source=PREFERRED_SOURCE)))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--apply', action='store_true', help='Apply the saved plan with a database backup; publish a new snapshot afterward')
    run(parser.parse_args())
