#!/usr/bin/env python3
"""Compare a complete observed Standard Ebooks listing with the finished catalog."""
import argparse
from collections import defaultdict
import csv
import json
from pathlib import Path
import re
import sqlite3
import unicodedata


def normalized(value):
    return ' '.join(re.findall(r'\w+', unicodedata.normalize('NFKC', value).casefold()))


def work_key(title, authors):
    return normalized(title), tuple(sorted(normalized(author) for author in authors))


def audit(db, manifest):
    present = set()
    for source, identity, url in db.execute("SELECT source,entry_id,entry_url FROM book_source UNION ALL SELECT source,entry_id,entry_url FROM external_book WHERE format=1"):
        if source == 'standardebooks.org':
            present.update(value.rstrip('/') for value in (identity, url) if value)
    works = defaultdict(list)
    sources = defaultdict(set)
    for key, source in db.execute('SELECT hash,source FROM book_source UNION ALL SELECT entry_key,source FROM external_book WHERE format=1'):
        sources[key].add(source)
    for kind, key, title, author, raw in db.execute("SELECT 'book',hash,title,author,record FROM book UNION ALL SELECT 'external_book',entry_key,title,author,record FROM external_book WHERE format=1"):
        record = json.loads(raw)
        authors = [credit['name'] for credit in record.get('contributors', []) if credit.get('role') in ([97, 117, 116], 'aut')]
        if not authors and author:
            authors = [author]
        works[work_key(title, authors)].append({'kind': kind, 'id': key, 'title': title, 'author': author, 'sources': sorted(sources[key])})
    missing = []
    for entry in manifest['entries']:
        if entry['entry_id'].rstrip('/') in present:
            continue
        matches = works.get(work_key(entry['title'], entry['authors']), [])
        missing.append({**entry, 'matching_catalog_epubs': matches,
                        'status': 'other_catalog_edition_present' if matches else 'no_exact_title_author_match'})
    return {'standardebooks_total': manifest['total'], 'standardebooks_editions_present': manifest['total'] - len(missing),
            'missing_standardebooks_editions': len(missing),
            'missing_editions_with_other_catalog_epub': sum(bool(entry['matching_catalog_epubs']) for entry in missing),
            'missing_editions_without_exact_title_author_match': sum(not entry['matching_catalog_epubs'] for entry in missing),
            'matching_rule': 'Canonical source URL for edition presence; Unicode-normalized title and sorted author names for other EPUB editions; approximate aliases are not treated as confirmed matches',
            'listing_url': manifest['listing_url'], 'observed_at': manifest['observed_at'], 'missing': missing}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--ranking', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(args.ranking.read_text())
    with sqlite3.connect((args.catalog / 'curation.sqlite3').resolve().as_uri() + '?mode=ro', uri=True) as db:
        result = audit(db, manifest)
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'missing-standardebooks.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    with (args.output / 'missing-standardebooks.csv').open('w', newline='') as output:
        writer = csv.DictWriter(output, fieldnames=['rank', 'title', 'authors', 'entry_id', 'status', 'matching_catalog_epubs'])
        writer.writeheader()
        for entry in result['missing']:
            writer.writerow({**entry, 'authors': ', '.join(entry['authors']),
                             'matching_catalog_epubs': json.dumps(entry['matching_catalog_epubs'], ensure_ascii=False)})
    print(json.dumps({key: value for key, value in result.items() if key != 'missing'}, ensure_ascii=False))
