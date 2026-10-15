#!/usr/bin/env python3
"""Read-only audit of relaxed title/author matches and shared valid ISBNs."""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
from difflib import SequenceMatcher
import csv
import json
from pathlib import Path
import re
import sqlite3
import unicodedata

FORMATS = {1: 'EPUB', 2: 'PDF', 4: 'MOBI', 8: 'AZW', 16: 'Audiobook'}


def normalize(text):
    text = unicodedata.normalize('NFKD', text.casefold())
    text = ''.join(char for char in text if not unicodedata.combining(char))
    text = re.sub(r"['’‘ʼ]", '', text)
    return ' '.join(re.findall(r'\w+', text))


def title_key(title):
    return re.sub(r'^(?:the|an|a) ', '', normalize(title))


def number_labels(text):
    # Numbers, years, and standalone Roman numerals distinguish many volumes
    # and collections even when there is no explicit "volume" word.
    return re.findall(r'\b(?:\d+|[IVXLCDM]+)\b', text)


def canonical_isbn(value):
    text = re.sub(r'^ISBN(?:-1[03])?\s*:?\s*', '', value.strip(), flags=re.I)
    text = re.sub(r'[\s\-–—]', '', text).upper()
    if re.fullmatch(r'\d{9}[\dX]', text):
        digits = [int(char) if char != 'X' else 10 for char in text]
        if sum((10 - index) * digit for index, digit in enumerate(digits)) % 11:
            return None
        body = '978' + text[:9]
        return body + str((-sum(int(char) * (1 if index % 2 == 0 else 3) for index, char in enumerate(body))) % 10)
    if re.fullmatch(r'97[89]\d{10}', text):
        if sum(int(char) * (1 if index % 2 == 0 else 3) for index, char in enumerate(text)) % 10 == 0:
            return text
    return None


def read_entries(db):
    published = bool(db.execute("SELECT 1 FROM sqlite_master WHERE name='snapshot'").fetchone())
    sources = defaultdict(list)
    if published:
        for key, source in db.execute('SELECT hash,name FROM book_source JOIN source ON source.id=source_id UNION ALL SELECT entry_key,name FROM external_book JOIN source ON source.id=source_id'):
            sources[key].append(source)
    else:
        for key, source in db.execute('SELECT hash,source FROM book_source UNION ALL SELECT entry_key,source FROM external_book'):
            sources[key].append(source)
    rows = db.execute("SELECT 'hosted',hash,1,title,subtitle,author,record FROM book UNION ALL SELECT 'external',entry_key,format,title,subtitle,author,record FROM external_book")
    entries = []
    invalid = []
    isbn_occurrences = 0
    for kind, key, format_code, title, subtitle, author, raw in rows:
        record = json.loads(raw)
        credits = [c for c in record.get('contributors', []) if c.get('role') in ([97, 117, 116], 'aut')]
        ids = sorted({c['contributor_id'] for c in credits if c.get('contributor_id')})
        names = sorted({normalize(c['name']) for c in credits if c.get('name')})
        isbn_values = defaultdict(list)
        for identifier in record.get('book', {}).get('identifiers', []):
            scheme = identifier.get('scheme')
            if scheme != 'isbn' and not (isinstance(scheme, dict) and str(scheme.get('other', '')).casefold() == 'isbn'):
                continue
            isbn_occurrences += 1
            value = identifier.get('value', '')
            isbn = canonical_isbn(value)
            if isbn:
                isbn_values[isbn].append(dict(value=value, scope=identifier.get('scope')))
            else:
                invalid.append(dict(id=key, title=title, author=author, format=FORMATS[format_code], value=value, scope=identifier.get('scope')))
        full_title = title + ' ' + (subtitle or '')
        volumes = re.findall(r'\b(?:volumes?|vols?|v|books?|parts?|tomes?|delen|band)\.?\s+(?:\d+(?:\s*(?:and|[-–])\s*\d+)?|[ivxlcdm]+)\b', full_title.casefold())
        volumes += re.findall(r'\b(?:första|andra|tredje|fjärde|femte|sjätte)\s+delen\b', full_title.casefold())
        entry = dict(kind=kind, id=key, format_code=format_code, format=FORMATS[format_code], title=title, subtitle=subtitle, author=author,
                     author_ids=ids, author_names=names, languages=record.get('book', {}).get('languages', []),
                     sources=sorted(set(sources[key])), isbns=dict(isbn_values), volume_labels=volumes, number_labels=number_labels(full_title))
        entries.append(entry)
    return sorted(entries, key=lambda entry: (entry['format_code'], entry['id'])), invalid, isbn_occurrences


def author_key(entry):
    if entry['author_ids']:
        return 'identities', tuple(entry['author_ids'])
    if entry['author_names']:
        return 'names', tuple(entry['author_names'])
    return 'displayed', normalize(entry['author']) or entry['id']


def flags(entries):
    return dict(different_titles=len({e['title'] for e in entries}) > 1,
                different_displayed_authors=len({e['author'] for e in entries}) > 1,
                different_subtitles=len({e['subtitle'] for e in entries}) > 1,
                different_languages=len({tuple(sorted(lang.split('-')[0] for lang in e['languages'])) for e in entries}) > 1,
                different_volume_labels=len({tuple(e['volume_labels']) for e in entries}) > 1,
                different_number_labels=len({tuple(e['number_labels']) for e in entries}) > 1)


def audit(database, output):
    output.mkdir(parents=True, exist_ok=False)
    # Pin a consistent database image while other importers continue running.
    with sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True) as source, sqlite3.connect(output / 'audited.sqlite3') as image:
        source.backup(image)
    with sqlite3.connect((output / 'audited.sqlite3').resolve().as_uri() + '?mode=ro', uri=True) as db:
        entries, invalid, occurrences = read_entries(db)
        published = db.execute("SELECT 1 FROM sqlite_master WHERE name='snapshot'").fetchone()
        release = db.execute('SELECT release_id FROM snapshot').fetchone()[0] if published else None
    exact = defaultdict(list)
    relaxed = defaultdict(list)
    by_author = defaultdict(list)
    by_isbn = defaultdict(list)
    for entry in entries:
        exact[entry['format_code'], entry['title'], entry['author']].append(entry)
        key = author_key(entry)
        relaxed[entry['format_code'], title_key(entry['title']), key].append(entry)
        by_author[entry['format_code'], key].append(entry)
        for isbn in entry['isbns']:
            by_isbn[isbn].append(entry)
    relaxed_groups = []
    for key, group in sorted(relaxed.items()):
        if len(group) < 2:
            continue
        relaxed_groups.append(dict(format=FORMATS[key[0]], normalized_title=key[1], count=len(group),
                                   beyond_exact=bool(len({(e['title'], e['author']) for e in group}) > 1),
                                   flags=flags(group), entries=group))
    fuzzy = []
    for group in by_author.values():
        for index, left in enumerate(group):
            a = title_key(left['title'])
            for right in group[index + 1:]:
                b = title_key(right['title'])
                if a == b or min(len(a), len(b)) < 10:
                    continue
                ratio = SequenceMatcher(None, a, b, autojunk=False).ratio()
                short, long = sorted((a, b), key=len)
                prefix = len(short) >= 15 and long.startswith(short + ' ') and len(short) / len(long) >= 0.6
                if ratio >= 0.9 or prefix:
                    fuzzy.append(dict(format=left['format'], similarity=round(ratio, 4), reason='title_prefix' if prefix else 'title_similarity',
                                      flags=flags([left, right]), entries=[left, right]))
    isbn_groups = []
    for isbn, group in sorted(by_isbn.items()):
        if len(group) < 2:
            continue
        formats = Counter(e['format'] for e in group)
        isbn_groups.append(dict(isbn13=isbn, count=len(group), same_format_overlap=any(count > 1 for count in formats.values()),
                                formats=dict(formats), flags=flags(group), entries=group))
    summary = dict(database=str(database), checked_at=datetime.now(timezone.utc).isoformat(), release_id=release, total_entries=len(entries),
                   exact_groups=sum(len(group) > 1 for group in exact.values()),
                   relaxed_groups=len(relaxed_groups), relaxed_groups_beyond_exact=sum(g['beyond_exact'] for g in relaxed_groups),
                   relaxed_groups_beyond_exact_by_format=dict(Counter(g['format'] for g in relaxed_groups if g['beyond_exact'])),
                   fuzzy_title_pairs=len(fuzzy), fuzzy_pairs_with_volume_differences=sum(g['flags']['different_volume_labels'] for g in fuzzy),
                   fuzzy_pairs_with_number_or_volume_differences=sum(g['flags']['different_volume_labels'] or g['flags']['different_number_labels'] for g in fuzzy),
                   isbn_occurrences=occurrences, entries_with_valid_isbn=sum(bool(e['isbns']) for e in entries),
                   invalid_isbn_occurrences=len(invalid), overlapping_isbn_groups=len(isbn_groups),
                   same_format_isbn_groups=sum(g['same_format_overlap'] for g in isbn_groups),
                   cross_format_only_isbn_groups=sum(not g['same_format_overlap'] for g in isbn_groups),
                   same_format_isbn_groups_by_format={name: sum(g['formats'].get(name, 0) > 1 for g in isbn_groups) for name in FORMATS.values()},
                   overlapping_isbn_groups_with_different_titles=sum(g['flags']['different_titles'] for g in isbn_groups))
    report = dict(summary=summary, relaxed_matching='Same media format; shared author identities (or normalized author names); titles normalized for case, punctuation, accents, and leading a/an/the.',
                  fuzzy_matching='Same media format and author identities; title similarity >=0.90 or whole-word prefix covering >=60% of the longer title. Review candidates only; volume and language differences flagged.',
                  isbn_matching='Only checksum-valid ISBNs; ISBN-10 converted to ISBN-13; separators removed; repeated ISBNs within one entry counted once. Different media formats remain separate.',
                  relaxed_groups=relaxed_groups, fuzzy_pairs=fuzzy, isbn_groups=isbn_groups, invalid_isbns=invalid)
    (output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    for filename, groups in [('relaxed-titles.csv', relaxed_groups), ('fuzzy-titles.csv', fuzzy),
                             ('shared-isbns.csv', [g for g in isbn_groups if g['same_format_overlap']]),
                             ('cross-format-isbns.csv', [g for g in isbn_groups if not g['same_format_overlap']])]:
        with (output / filename).open('w', newline='', encoding='utf-8') as file:
            writer = csv.DictWriter(file, fieldnames=['group', 'match', 'format', 'id', 'title', 'subtitle', 'author', 'languages', 'sources', 'isbns', 'flags'])
            writer.writeheader()
            for index, group in enumerate(groups, 1):
                for entry in group['entries']:
                    writer.writerow(dict(group=index, match=group.get('isbn13', group.get('normalized_title', group.get('reason'))),
                                         format=entry['format'], id=entry['id'], title=entry['title'], subtitle=entry['subtitle'], author=entry['author'],
                                         languages=json.dumps(entry['languages'], ensure_ascii=False), sources=json.dumps(entry['sources'], ensure_ascii=False),
                                         isbns='; '.join(entry['isbns']), flags=json.dumps(group['flags'])))
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    audit(args.database, args.output)
