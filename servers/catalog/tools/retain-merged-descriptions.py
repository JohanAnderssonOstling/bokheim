#!/usr/bin/env python3
"""Restore component descriptions on generated books without rewriting EPUB assets."""
import argparse
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sqlite3
import sys
from urllib.parse import urlsplit

from bs4 import BeautifulSoup


def module():
    spec = importlib.util.spec_from_file_location('description_merger', Path(__file__).with_name('merge-epubs.py'))
    value = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = value
    spec.loader.exec_module(value)
    return value


def page_description(path):
    raw = Path(path).read_bytes()
    html = BeautifulSoup(raw, 'html.parser')
    summary = html.select_one('.summary-text-container')
    if summary is None:
        for row in html.select('table.bibrec tr'):
            heading, cell = row.find('th'), row.find('td')
            if heading and cell and heading.get_text(' ', strip=True).casefold() == 'summary':
                summary = cell
                break
    if summary is None:
        return '', None
    for control in summary.select('label, script, style, .readmore, .showless, .readmoredots'):
        control.decompose()
    text = ' '.join(summary.get_text(' ', strip=True).split())
    return text, {'page_file': str(path), 'page_sha256': hashlib.sha256(raw).hexdigest()}


def recovery(metadata, pages):
    descriptions, evidence = [], []
    for component in metadata['components']:
        description = component['record'].get('description', '')
        source = {'component_hash': component['hash'], 'label': component.get('label', '')}
        if description.strip():
            source['kind'] = 'retained_component_record'
        else:
            for entry in component['source_entries']:
                if entry['source'] != 'gutenberg.org':
                    continue
                if entry['entry_url'] in pages:
                    description, page = page_description(pages[entry['entry_url']])
                    if description:
                        source.update(kind='retained_gutenberg_page', page_url=entry['entry_url'], **page)
                        break
                description = entry['metadata'].get('source_metadata', {}).get('description', '')
                if description.strip():
                    source.update(kind='retained_source_metadata', page_url=entry['entry_url'])
                    break
        if description.strip():
            descriptions.append((component.get('label', ''), description))
            evidence.append(dict(source, description=description))
    return module().combine_descriptions(descriptions), evidence


def restore(db, pages):
    changes, skipped = [], []
    db.execute('BEGIN IMMEDIATE')
    try:
        for identity, book_hash, raw, record_raw in db.execute("SELECT s.entry_id,s.book_hash,s.metadata,b.record FROM source_entry s JOIN book b ON b.hash=s.book_hash WHERE s.source='bokheim:combined-volumes'").fetchall():
            metadata, record = json.loads(raw), json.loads(record_raw)
            previous = record.get('description', '')
            description, sources = recovery(metadata, pages)
            if not description or description == previous:
                continue
            prior = metadata.get('description_retention', {}).get('description')
            if previous.strip() and not previous.startswith('Combined edition of ') and previous != prior:
                skipped.append({'hash': book_hash, 'reason': 'preserve_existing_description'})
                continue
            changes.append({'hash': book_hash, 'entry_id': identity, 'title': record['title'], 'previous_description': previous, 'description': description, 'sources': sources})
            record['description'] = description
            metadata['description_retention'] = {'policy': 'preserve_distinct_component_descriptions', 'description': description, 'sources': sources}
            db.execute('UPDATE book SET record=? WHERE hash=?', [json.dumps(record, ensure_ascii=False), book_hash])
            db.execute("UPDATE source_entry SET metadata=? WHERE source='bokheim:combined-volumes' AND entry_id=?", [json.dumps(metadata, ensure_ascii=False), identity])
        db.commit()
    except Exception:
        db.rollback()
        raise
    return {'updated': len(changes), 'changes': changes, 'skipped': skipped}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--pages-manifest', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    pages = {}
    if args.pages_manifest:
        for row in json.loads(args.pages_manifest.read_text()):
            url = urlsplit(row['page_url'])
            if url.scheme != 'https' or url.netloc != 'www.gutenberg.org' or not url.path.removeprefix('/ebooks/').isdigit() or url.query or url.fragment:
                raise ValueError('Description pages require canonical Gutenberg URLs')
            pages[row['page_url']] = Path(row['page_file'])
    receipt = args.output / 'description-retention.json'
    if receipt.exists():
        raise ValueError('Use a fresh audit directory for each restoration pass')
    with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db:
        backup_path = args.output / 'curation-before-descriptions.sqlite3'
        with sqlite3.connect(backup_path) as backup:
            db.backup(backup)
        result = restore(db, pages)
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not db.execute('PRAGMA foreign_key_check').fetchall()
    receipt.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    with backup_path.open('rb') as source, gzip.open(str(backup_path) + '.gz', 'wb') as destination:
        shutil.copyfileobj(source, destination)
    with backup_path.open('rb') as source, gzip.open(str(backup_path) + '.gz', 'rb') as restored:
        assert hashlib.file_digest(source, 'sha256').digest() == hashlib.file_digest(restored, 'sha256').digest()
    backup_path.unlink()
    print(json.dumps({'updated': result['updated'], 'skipped': len(result['skipped'])}))


if __name__ == '__main__':
    main()
