#!/usr/bin/env python3
"""Import Standard Ebooks ZIP members in memory, enrich, retain covers and replace exact matches."""
import argparse
from datetime import datetime, timezone
import hashlib
import gzip
import importlib.util
import io
import json
from pathlib import Path
import sqlite3
import shutil
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import xml.etree.ElementTree as ET
import zipfile


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    loaded = importlib.util.module_from_spec(spec)
    sys.modules[name] = loaded
    spec.loader.exec_module(loaded)
    return loaded


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


def inventory(downloads):
    entries = {}
    for path in sorted(downloads.glob('*.zip')):
        if 'se-ebooks' not in path.name.casefold():
            continue
        with zipfile.ZipFile(path) as archive:
            for member in archive.namelist():
                if not member.casefold().endswith('.epub'):
                    continue
                data = archive.read(member)
                digest = hashlib.sha256(data).hexdigest()
                observation = {'archive': str(path.resolve()), 'member': member}
                if digest in entries:
                    entries[digest]['source_evidence']['archive_observations'].append(observation)
                    continue
                with zipfile.ZipFile(io.BytesIO(data)) as epub:
                    package = ET.fromstring(epub.read('META-INF/container.xml')).find('.//{*}rootfile').get('full-path')
                    metadata = ET.fromstring(epub.read(package)).find('{*}metadata')
                    identifiers = [node.text or '' for node in metadata.findall('{*}identifier')]
                    url = next((value.removeprefix('url:') for value in identifiers
                                if value.removeprefix('url:').startswith('https://standardebooks.org/ebooks/')), None)
                    if not url:
                        raise ValueError('Missing Standard Ebooks identity: ' + member)
                    entries[digest] = {'file': str(path.resolve()), 'archive_member': member,
                        'preserve_covers': True, 'source': 'standardebooks.org', 'entry_id': url,
                        'entry_url': url, 'file_url': url + '/downloads/' + Path(member).name,
                        'source_evidence': {'archive_observations': [observation], 'sha256': digest,
                            'title': metadata.findtext('{*}title'),
                            'authors': [node.text or '' for node in metadata.findall('{*}creator')],
                            'cover_policy': 'retain existing catalog artwork; embedded cover for new books; no cover enrichment'}}
    return list(entries.values())


def replacement_plan(db, catalog, imported):
    dedup = module('standardebooks_dedup', 'deduplicate-catalog.py')
    # Restrict the established tool to imported exact title/author groups.
    # Reviewed aliases remain available for ordinary curation, not this import.
    dedup.REVIEWED_TITLE_ALIASES = {}
    decisions = []
    for group in dedup.plan(db, catalog):
        rows = [group['kept']] + group['removed']
        incoming = [row for row in rows if row['kind'] == 'book' and row['id'] in imported]
        if not incoming:
            continue
        preferred = [row for row in rows if (
            db.execute('SELECT 1 FROM book_source WHERE hash=? AND source IN (?,?)',
                       (row['id'], dedup.PREFERRED_SOURCE, 'standardebooks.org')).fetchone() if row['kind'] == 'book'
            else db.execute('SELECT 1 FROM external_book WHERE entry_key=? AND source IN (?,?)',
                            (row['id'], dedup.PREFERRED_SOURCE, 'standardebooks.org')).fetchone())]
        kept = incoming[0]
        group['kept'] = kept
        group['removed'] = [row for row in rows if row not in incoming and row not in preferred]
        if not group['removed']:
            continue
        group['kept_sources'] = [r[0] for r in db.execute('SELECT source FROM book_source WHERE hash=?', (kept['id'],))] if kept['kind'] == 'book' else [dedup.PREFERRED_SOURCE]
        decisions.append(group)
    return dedup, decisions


def run(args):
    args.output.mkdir(parents=True, exist_ok=True)
    if args.popularity_only:
        manifest = json.loads(args.popularity_only.read_text())
        popularity = module('standardebooks_ordinal_popularity', 'standardebooks-popularity.py')
        with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=60) as db:
            db.execute('BEGIN IMMEDIATE')
            updated = popularity.apply(db, manifest)
            db.commit()
        write_json(args.output / 'popularity-ranking.json', manifest)
        publisher = module('standardebooks_rank_publish', 'run-volume-merge-pass.py')
        publisher.publish(SimpleNamespace(catalog=args.catalog, binary=args.binary, output=args.output))
        print(json.dumps({'state': 'popularity-published', 'ranked_imported_books': updated, 'ranking_total': manifest['total']}), flush=True)
        return
    inventory_path = args.output / 'inventory.json'
    if args.manifest:
        entries = json.loads(args.manifest.read_text())
        write_json(inventory_path, entries)
    elif inventory_path.exists():
        entries = json.loads(inventory_path.read_text())
        # Include archives added while the import was in progress.
        latest = inventory(args.downloads)
        if latest != entries:
            entries = latest
            write_json(inventory_path, entries)
    else:
        entries = inventory(args.downloads)
        write_json(inventory_path, entries)
    if not entries:
        raise ValueError('No Standard Ebooks EPUBs found')
    if args.manifest:
        print(f'Acquisition manifest: {len(entries)} EPUB downloads', flush=True)
    else:
        print(f'Inventory: {len(entries)} distinct EPUB contents from {len(set(o["archive"] for e in entries for o in e["source_evidence"]["archive_observations"]))} ZIPs', flush=True)
    db_path = args.catalog / 'curation.sqlite3'
    backup = module('standardebooks_backup', 'import-merged-epub.py')
    backup_path = args.output / 'curation-before.sqlite3.gz'
    if not backup_path.exists():
        with tempfile.TemporaryDirectory(prefix='standardebooks-backup-') as temporary:
            raw = Path(temporary) / 'curation.sqlite3'
            with sqlite3.connect(db_path, timeout=60) as db:
                backup.backup_catalog(db, raw)
            partial = backup_path.with_suffix('.gz.partial')
            with raw.open('rb') as source, gzip.open(partial, 'wb') as destination:
                shutil.copyfileobj(source, destination)
            with gzip.open(partial, 'rb') as compressed:
                if hashlib.file_digest(compressed, 'sha256').digest() != hashlib.sha256(raw.read_bytes()).digest():
                    raise ValueError('Compressed backup verification failed')
            partial.replace(backup_path)
    covers_path = args.output / 'original-covers.json'
    if not covers_path.exists():
        write_json(covers_path, {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                                for p in (args.catalog / 'covers').glob('*.jpg')})
    progress_path = args.output / 'progress.json'
    progress = json.loads(progress_path.read_text()) if progress_path.exists() else {}
    failures = []
    for index, entry in enumerate(entries, 1):
        digest = entry['source_evidence'].get('sha256') or entry['entry_id']
        name = entry.get('archive_member') or Path(entry['file']).name
        if progress.get(digest, {}).get('state') == 'imported':
            continue
        for _ in range(6):
            if shutil.disk_usage(args.catalog).free >= 100 * 1024 * 1024:
                break
            time.sleep(5)
        else:
            raise RuntimeError('Less than 100 MiB remains; import can resume after freeing disk space')
        manifest = args.output / 'current-manifest.json'
        write_json(manifest, [entry])
        result = None
        for _ in range(3):
            result = subprocess.run([str(args.binary), str(args.catalog), 'import-manifest', str(manifest)],
                                    capture_output=True, text=True)
            if result.returncode == 0 or '429 Too Many Requests' in result.stderr:
                break
        with (args.output / 'import.log').open('a') as log:
            log.write(name + '\n' + result.stdout + result.stderr + '\n')
        if result.returncode:
            progress[digest] = {'state': 'failed', 'member': name, 'error': result.stderr}
            failures.append(name)
        else:
            with sqlite3.connect(db_path, timeout=60) as db:
                row = db.execute('SELECT book_hash FROM source_entry WHERE source=? AND entry_id=?',
                                 (entry['source'], entry['entry_id'])).fetchone()
                if not row or not row[0]:
                    raise ValueError('Import did not retain acquisition evidence')
            progress[digest] = {'state': 'imported', 'member': name, 'hash': row[0]}
        write_json(progress_path, progress)
        print(f'{index}/{len(entries)} {progress[digest]["state"]}: {name}', flush=True)
        if result.returncode and '429 Too Many Requests' in result.stderr:
            raise RuntimeError('Source rate limit reached; progress saved. Wait for the limit to reset before resuming')
    if failures:
        raise RuntimeError(f'{len(failures)} imports failed; resume with the same output directory')
    imported = {progress[e['source_evidence'].get('sha256') or e['entry_id']]['hash'] for e in entries}
    receipt_path = args.output / 'replacement-receipt.json'
    previous_receipt = json.loads(receipt_path.read_text()) if receipt_path.exists() else None
    if previous_receipt is None or previous_receipt['imported_contents'] != len(entries):
        if previous_receipt is not None:
            write_json(args.output / f'replacement-receipt-{previous_receipt["imported_contents"]}.json', previous_receipt)
        with sqlite3.connect(db_path, timeout=60) as db:
            db.execute('PRAGMA foreign_keys=ON')
            db.execute('BEGIN IMMEDIATE')
            dedup, decisions = replacement_plan(db, args.catalog, imported)
            retained = {('book', row[0]) for row in db.execute('SELECT hash FROM book') if row[0] in imported}
            retained.update(('book', row[0]) for row in db.execute('SELECT hash FROM book_source WHERE source IN (?,?)', (dedup.PREFERRED_SOURCE, 'standardebooks.org')))
            retained.update(('external_book', row[0]) for row in db.execute('SELECT entry_key FROM external_book WHERE source IN (?,?)', (dedup.PREFERRED_SOURCE, 'standardebooks.org')))
            write_json(args.output / 'replacement-plan.json', decisions)
            # Preserve external-link provenance when replacing one with a hosted EPUB.
            for group in decisions:
                if group['kept']['kind'] != 'book':
                    continue
                for row in group['removed']:
                    if row['kind'] == 'external_book':
                        db.execute('INSERT OR IGNORE INTO book_source(hash,source,entry_id,entry_url,file_url) SELECT ?,source,entry_id,entry_url,file_url FROM external_book WHERE entry_key=?',
                                   (group['kept']['id'], row['id']))
                        db.execute('UPDATE source_entry SET book_hash=?,external_entry_key=NULL WHERE external_entry_key=?',
                                   (group['kept']['id'], row['id']))
            dedup.apply(db, decisions, args.catalog, retain_entries=retained)
            db.commit()
        receipt = {'groups': len(decisions), 'removed_entries': sum(len(g['removed']) for g in decisions),
                   'protected_groups': sum(dedup.PREFERRED_SOURCE in g['kept_sources'] for g in decisions),
                   'imported_contents': len(entries), 'original_files_retained': True,
                   'incoming_entries_discarded': 0,
                   'existing_duplicate_entries_removed': sum(len(g['removed']) for g in decisions),
                   'applied_at': datetime.now(timezone.utc).isoformat()}
        write_json(receipt_path, receipt)
    original_covers = json.loads(covers_path.read_text())
    changed = [name for name, digest in original_covers.items()
               if hashlib.sha256((args.catalog / 'covers' / name).read_bytes()).hexdigest() != digest]
    if changed:
        raise RuntimeError('Existing covers changed: ' + repr(changed))
    publisher = module('standardebooks_publish', 'run-volume-merge-pass.py')
    publisher.publish(SimpleNamespace(catalog=args.catalog, binary=args.binary, output=args.output))
    changed = [name for name, digest in original_covers.items()
               if hashlib.sha256((args.catalog / 'covers' / name).read_bytes()).hexdigest() != digest]
    if changed:
        raise RuntimeError('Publication changed existing covers: ' + repr(changed))
    print(json.dumps({'state': 'complete', **json.loads(receipt_path.read_text()),
                      'existing_covers_verified_unchanged': len(original_covers)}), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--downloads', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--popularity-only', type=Path, help='Apply a validated observed ranking and publish, without reimporting books')
    parser.add_argument('--manifest', type=Path, help='Import explicit acquisition entries instead of scanning local ZIPs')
    run(parser.parse_args())
