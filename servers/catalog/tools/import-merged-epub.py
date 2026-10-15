#!/usr/bin/env python3
"""Inspect a validated merge, then add its record and component provenance locally."""
import argparse
from contextlib import closing
from copy import deepcopy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unicodedata


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    result = importlib.util.module_from_spec(spec)
    sys.modules[name] = result
    spec.loader.exec_module(result)
    return result


def unique(values):
    result = []
    for value in values:
        if value not in result:
            result.append(deepcopy(value))
    return result


def backup_catalog(db, path):
    """Expose a backup only after SQLite finishes and its catalog is readable."""
    def verify(candidate):
        try:
            with closing(sqlite3.connect(f'file:{candidate}?mode=ro', uri=True)) as backup:
                if backup.execute('PRAGMA integrity_check').fetchone()[0] != 'ok':
                    raise ValueError('Catalog backup failed its integrity check')
                # A rolled-back, interrupted backup can be an empty but valid
                # SQLite database. Integrity alone does not establish a backup.
                backup.execute('SELECT hash,record FROM book LIMIT 1').fetchall()
        except sqlite3.Error as error:
            raise ValueError('Catalog backup is incomplete or unreadable') from error

    path = Path(path)
    if path.exists():
        verify(path)
        return
    with tempfile.TemporaryDirectory(prefix='catalog-backup-', dir=path.parent) as temporary:
        candidate = Path(temporary) / 'curation.sqlite3'
        with closing(sqlite3.connect(candidate)) as backup:
            db.backup(backup)
        verify(candidate)
        try:
            os.link(candidate, path)
        except FileExistsError:
            # Another importer may have completed the shared backup first.
            verify(path)


def combined_record(inspected, components, title, additional_credits=None):
    """Edition identity is new; original ISBNs remain associated with components."""
    record = deepcopy(inspected)
    records = [c['record'] for c in components]
    record['title'] = title
    record['subtitle'] = None
    merger = module('merge_epubs_descriptions', 'merge-epubs.py')
    retained = merger.combine_descriptions((component.get('label', ''), component['record'].get('description', '')) for component in components)
    record['description'] = retained or inspected.get('description', '')
    record['contributors'] = []
    credited = set()
    for source in records:
        for credit in source['contributors']:
            key = (' '.join(unicodedata.normalize('NFKC', credit['name']).casefold().split()), tuple(credit['role']))
            if key not in credited:
                record['contributors'].append(deepcopy(credit))
                credited.add(key)
    for extra in additional_credits or []:
        matches = [c for c in inspected.get('contributors', [])
                   if c['name'] == extra['name'] and bytes(c['role']).decode() == extra['role']]
        if not matches:
            raise ValueError('Reviewed contributor was not retained by the EPUB inspector')
        credit = matches[0]
        key = (' '.join(unicodedata.normalize('NFKC', credit['name']).casefold().split()), tuple(credit['role']))
        if key not in credited:
            record['contributors'].append(deepcopy(credit))
            credited.add(key)
    for field in ['subjects', 'publishers', 'languages']:
        record['book'][field] = unique(value for r in records for value in r['book'][field])
    years = {r['book']['year'] for r in records}
    record['book']['year'] = next(iter(years)) if len(years) == 1 else None
    return record


def run(args):
    report = json.loads(args.report.read_text())
    epub = Path(report['output'])
    if hashlib.sha256(epub.read_bytes()).hexdigest() != report['sha256']:
        raise ValueError('The merged EPUB differs from its validation report')
    merger = module('merge_epubs_import', 'merge-epubs.py')
    merger.validate_epub(epub)
    if report.get('review') is not None:
        volumes = [merger.read_volume(v['path'], v['label'], i) for i, v in enumerate(report['volumes'], 1)]
        merger.validate_review(volumes, report['title'], report['review'])
    db_path = args.catalog / 'curation.sqlite3'
    components = []
    prior_metadata = None
    prior_raw = None
    with sqlite3.connect(f'file:{db_path}?mode=ro', uri=True) as db:
        if args.replace_merged:
            prior = db.execute('SELECT metadata FROM source_entry WHERE source=? AND book_hash=?',
                               ['bokheim:combined-volumes', args.replace_merged]).fetchone()
            if not prior:
                raise ValueError('Replacement must identify an existing generated combined edition')
            prior_raw = prior[0]
            prior_metadata = json.loads(prior_raw)
            expected = [(Path(v['path']).stem, v['sha256']) for v in report['volumes']]
            if [(v['hash'], v['sha256']) for v in prior_metadata['components']] != expected:
                raise ValueError('Replacement must retain exactly the same ordered source hashes and bytes')
        for volume in report['volumes']:
            path = Path(volume['path'])
            if path.resolve() != (args.catalog / 'books' / path.name).resolve():
                raise ValueError('Every component must be a retained book in this catalog')
            if hashlib.sha256(path.read_bytes()).hexdigest() != volume['sha256']:
                raise ValueError('A component EPUB changed after merging')
            row = db.execute('SELECT record FROM book WHERE hash=?', [path.stem]).fetchone()
            if not row:
                if prior_metadata is None:
                    raise ValueError('A component has no catalog record')
                # Archived originals remain valid inputs only for an explicitly
                # identified replacement bound to their original content hashes.
                component = deepcopy(next(c for c in prior_metadata['components'] if c['hash'] == path.stem))
                component['label'] = volume['label']
                components.append(component)
                continue
            entries = [{'source': source, 'entry_id': entry_id, 'entry_url': url, 'metadata': json.loads(metadata)}
                       for source, entry_id, url, metadata in db.execute('SELECT source,entry_id,entry_url,metadata FROM source_entry WHERE book_hash=?', [path.stem])]
            component = deepcopy(next(c for c in prior_metadata['components'] if c['hash'] == path.stem)) if prior_metadata else {}
            known = {(e['source'], e['entry_id']) for e in entries}
            entries.extend(e for e in component.get('source_entries', [])
                           if (e['source'], e['entry_id']) not in known)
            component.update({'hash': path.stem, 'label': volume['label'], 'sha256': volume['sha256'],
                              'record': json.loads(row[0]), 'source_entries': entries})
            components.append(component)
        existing = db.execute('SELECT book_hash FROM source_entry WHERE source=? AND entry_id=?',
                              ['bokheim:combined-volumes', report['identifier']]).fetchone()
        if existing and existing[0] is not None:
            stored = args.catalog / 'books' / (existing[0] + '.epub')
            if hashlib.sha256(stored.read_bytes()).hexdigest() != report['sha256']:
                raise ValueError('This merge identifier already belongs to a different build')
            return {'hash': existing[0], 'state': 'already_imported'}
    # Use the Rust catalog inspector for the actual reader's TOC, content hash
    # and covers. Isolating its inbox avoids processing unrelated acquisitions.
    with tempfile.TemporaryDirectory(prefix='bokheim-merged-inspection-', dir=args.report.parent) as temporary:
        staging = Path(temporary)
        inbox = staging / 'inbox/bokheim-combined'
        inbox.mkdir(parents=True)
        shutil.copyfile(epub, inbox / epub.name)
        result = subprocess.run([str(args.binary), str(staging), 'import'], capture_output=True, text=True)
        (args.report.parent / 'inspection.log').write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError('Catalog inspection failed; see inspection.log')
        with sqlite3.connect(staging / 'curation.sqlite3') as db:
            rows = db.execute('SELECT hash,record,toc FROM book').fetchall()
        if len(rows) != 1 or rows[0][2] is None:
            raise ValueError('The catalog inspector did not produce exactly one book with navigation')
        content_hash, raw, toc = rows[0]
        inspected = json.loads(raw)
        record = combined_record(inspected, components, report['title'],
                                 (report.get('review') or {}).get('additional_credits'))
        if report.get('description_source') == 'catalog_components':
            description = report.get('description', '')
            if any(c['record'].get('description', '').strip() not in description
                   for c in components if c['record'].get('description', '').strip()):
                raise ValueError('Catalog description must retain every original component description')
            record['description'] = description
        # Preserve the merger's unique edition identity instead of borrowing an
        # ISBN or other edition identifiers found by title-based enrichment.
        record['book']['identifiers'] = [{'value': report['identifier'], 'scheme': 'unspecified', 'scope': 'edition'}]
        author = ', '.join(c['name'] for c in record['contributors'] if bytes(c['role']) == b'aut')
        backup_path = getattr(args, 'backup', None) or args.report.parent / 'curation-before-import.sqlite3'
        with sqlite3.connect(db_path, timeout=45) as db:
            backup_catalog(db, backup_path)
            # Files must be in place before the database exposes the edition.
            destination = args.catalog / 'books' / f'{content_hash}.epub'
            if destination.exists() and destination.read_bytes() != epub.read_bytes():
                raise ValueError('Content-hash destination has different bytes')
            if not destination.exists():
                try:
                    os.link(epub, destination)
                except OSError:
                    shutil.copyfile(epub, destination)
            for size in ['card', 'detail']:
                filename = f'{content_hash}-{size}.jpg'
                source = staging / 'covers' / filename
                destination_cover = args.catalog / 'covers' / filename
                if source.exists() and not destination_cover.exists():
                    shutil.copyfile(source, destination_cover)
            first_urls = [entry['entry_url'] for component in components for entry in component['source_entries']]
            if not first_urls:
                raise ValueError('The components need retained source-page provenance')
            evidence = {'kind': 'combined_volumes', 'merge': report, 'components': components,
                        'originals_retained': True, 'component_isbns_are_not_combined_edition_isbns': True}
            if prior_metadata is not None:
                evidence = {**deepcopy(prior_metadata), **evidence}
            db.execute('PRAGMA foreign_keys=ON')
            db.execute('BEGIN IMMEDIATE')
            # Recheck under the write lock for another importer of the same set.
            duplicate = db.execute('SELECT book_hash FROM source_entry WHERE source=? AND entry_id=?',
                                   ['bokheim:combined-volumes', report['identifier']]).fetchone()
            if duplicate and duplicate[0] is not None:
                raise ValueError('This merged edition was imported concurrently; retry to verify it')
            db.execute('INSERT INTO book(hash,title,subtitle,author,record,toc) VALUES(?,?,?,?,?,?)',
                       [content_hash, record['title'], None, author, json.dumps(record, ensure_ascii=False), toc])
            db.execute('INSERT INTO book_source(hash,source,entry_id,entry_url,file_url) VALUES(?,?,?,?,NULL)',
                       [content_hash, 'bokheim:combined-volumes', report['identifier'], first_urls[0]])
            db.execute('''INSERT INTO source_entry(source,entry_id,entry_url,book_hash,metadata) VALUES(?,?,?,?,?)
                          ON CONFLICT(source,entry_id) DO UPDATE SET entry_url=excluded.entry_url,
                          book_hash=excluded.book_hash,external_entry_key=NULL,metadata=excluded.metadata''',
                       ['bokheim:combined-volumes', report['identifier'], first_urls[0], content_hash, json.dumps(evidence, ensure_ascii=False)])
            if args.replace_merged:
                prior_sources = db.execute('SELECT source FROM book_source WHERE hash=?', [args.replace_merged]).fetchall()
                prior_evidence = db.execute('SELECT metadata FROM source_entry WHERE source=? AND book_hash=?',
                                           ['bokheim:combined-volumes', args.replace_merged]).fetchone()
                if not prior_sources or any(source != 'bokheim:combined-volumes' for source, in prior_sources) or not prior_evidence:
                    raise ValueError('Replacement must identify an existing generated combined edition')
                if prior_evidence[0] != prior_raw:
                    raise ValueError('Replacement provenance changed concurrently; retry the review')
                prior_components = json.loads(prior_evidence[0])['components']
                if [c['hash'] for c in prior_components] != [c['hash'] for c in components]:
                    raise ValueError('Replacement must contain exactly the same ordered source volumes')
                db.execute('DELETE FROM book WHERE hash=?', [args.replace_merged])
            db.commit()
    receipt = {'hash': content_hash, 'title': record['title'], 'state': 'imported', 'components': len(components),
               'originals_retained': True, 'toc_volume_headings': False}
    if args.replace_merged:
        receipt['replaced_generated_hash'] = args.replace_merged
    (args.report.parent / 'import-receipt.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--publish', action='store_true', help='Publish and install a new local catalog snapshot')
    parser.add_argument('--replace-merged', help='Replace a prior generated edition of the same volumes; retain its EPUB file')
    parser.add_argument('--backup', type=Path, help='Shared pre-import backup path for a batch')
    args = parser.parse_args()
    args.catalog = args.catalog.resolve()
    args.binary = args.binary.resolve()
    try:
        receipt = run(args)
        if args.publish:
            module('refine_subjects_publish', 'refine-subjects.py').publish(SimpleNamespace(catalog=args.catalog, binary=args.binary, output=args.report.parent))
    except (ValueError, KeyError, OSError, RuntimeError, sqlite3.Error) as error:
        parser.exit(1, f'Import failed: {error}\n')
    print(json.dumps(receipt, ensure_ascii=False))


if __name__ == '__main__':
    main()
