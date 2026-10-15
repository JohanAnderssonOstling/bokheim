#!/usr/bin/env python3
"""Resume an explicitly reviewed volume selection, validate, import and publish."""
import argparse
from collections import Counter
from datetime import datetime, timezone
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unicodedata


def now():
    return datetime.now(timezone.utc).isoformat(timespec='seconds')


def load_module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


def slug(title):
    text = unicodedata.normalize('NFKC', title).lower()
    name = ''.join(char if char.isalnum() else '-' for char in text)
    name = '-'.join(part for part in name.split('-') if part)
    while len(name.encode()) > 180:
        name = name[:-1]
    return name.rstrip('-') or 'book'


def existing_generated(catalog, hashes):
    with sqlite3.connect(f'file:{catalog}/curation.sqlite3?mode=ro', uri=True) as db:
        rows = db.execute("SELECT book_hash,metadata FROM source_entry WHERE source='bokheim:combined-volumes' AND book_hash IS NOT NULL").fetchall()
    found = [book_hash for book_hash, raw in rows if [c['hash'] for c in json.loads(raw).get('components', [])] == hashes]
    if len(found) > 1:
        raise ValueError('Several generated editions already exist for the same volumes')
    return found[0] if found else None


def source_failures(args, group, folder, failures):
    """Only quarantine errors reproduced at the same location in an input."""
    for failure in failures:
        for location in failure.get('locations', []):
            path = location.get('path', '')
            parts = path.split('/', 2)
            if len(parts) != 3 or parts[0] != 'volumes' or not parts[1].startswith('v'):
                return False
            index = int(parts[1][1:]) - 1
            if index < 0 or index >= len(group['volumes']):
                return False
            baseline = folder / f'source-v{index + 1:03}-epubcheck.json'
            if not baseline.exists():
                source = args.catalog / 'books' / (group['volumes'][index]['hash'] + '.epub')
                result = subprocess.run(['java', '-Xmx512m', '-jar', str(args.epubcheck), str(source), '--json', str(baseline)],
                                        capture_output=True, text=True, timeout=300)
                baseline.with_suffix('.log').write_text(result.stdout + result.stderr)
            if not baseline.exists():
                return False
            messages = json.loads(baseline.read_text())['messages']
            if not any(m['ID'] == failure['ID'] and m['message'] == failure['message']
                       and any(where.get('path') == parts[2] for where in m.get('locations', [])) for m in messages):
                return False
    return bool(failures)


def publish(args):
    root = args.catalog / 'published'
    for attempt in range(5):
        release = max((int(p.name) for p in (root / 'releases').iterdir() if p.name.isdigit()), default=0) + 1
        result = subprocess.run([str(args.binary), str(args.catalog), 'publish', str(root), str(release)], capture_output=True, text=True)
        if result.returncode:
            print('Publication retry: ' + result.stderr[-1000:], flush=True)
            continue
        snapshot = root / 'releases' / str(release) / 'catalog.sqlite3'
        with sqlite3.connect(f'file:{snapshot}?mode=ro', uri=True) as db:
            if db.execute('PRAGMA integrity_check').fetchone()[0] != 'ok' or db.execute('PRAGMA foreign_key_check').fetchall():
                raise ValueError('The published snapshot failed integrity checks')
        installed = args.catalog.parent / 'catalog.sqlite3'
        descriptor, filename = tempfile.mkstemp(prefix='catalog-merged-', suffix='.sqlite3', dir=installed.parent)
        os.close(descriptor)
        temporary = Path(filename)
        temporary.unlink()
        try:
            os.link(snapshot, temporary)
            # Avoid installing an older revision if another publisher advanced it.
            with sqlite3.connect(f'file:{installed}?mode=ro', uri=True) as current:
                if current.execute('SELECT release_id FROM snapshot').fetchone()[0] > release:
                    continue
            temporary.replace(installed)
        finally:
            temporary.unlink(missing_ok=True)
        write_json(args.output / 'last-publication.json', {'release_id': release, 'published_at': now(), 'installed': str(installed)})
        return
    raise RuntimeError('Could not publish the merged catalog after five attempts')


def one_group(args, group, merger, importer):
    hashes = [v['hash'] for v in group['volumes']]
    key = hashlib.sha256(json.dumps([group['title'], hashes]).encode()).hexdigest()[:12]
    folder = args.output / (slug(group['title']) + '-' + key)
    folder.mkdir(exist_ok=True)
    output = folder / (slug(group['title']) + '.epub')
    report_path = folder / 'merge-report.json'
    if not report_path.exists():
        paths = [args.catalog / 'books' / (h + '.epub') for h in hashes]
        labels = [v.get('label', f'Volume {i + 1}') for i, v in enumerate(group['volumes'])]
        with sqlite3.connect(f'file:{args.catalog}/curation.sqlite3?mode=ro', uri=True) as db:
            descriptions = [(label, json.loads(db.execute('SELECT record FROM book WHERE hash=?', [h]).fetchone()[0]).get('description', ''))
                            for h, label in zip(hashes, labels)]
        description = merger.combine_descriptions(descriptions) or None
        try:
            report = merger.merge_epubs(paths, output, group['title'], labels=labels, expand_sections=True, review=group.get('review'), description=description)
        except ValueError as error:
            if 'Numbered sections need a unique increasing sequence' not in str(error):
                raise
            report = merger.merge_epubs(paths, output, group['title'], labels=labels, review=group.get('review'), description=description)
            report['section_expansion_skipped'] = str(error)
        if group.get('completeness_review'):
            report['completeness_review'] = group['completeness_review']
        write_json(report_path, report)
    report = json.loads(report_path.read_text())
    if hashlib.sha256(output.read_bytes()).hexdigest() != report['sha256']:
        raise ValueError('The generated EPUB differs from its saved report')
    check_path = folder / 'epubcheck.json'
    if not check_path.exists():
        result = subprocess.run(['java', '-Xmx512m', '-jar', str(args.epubcheck), str(output), '--json', str(check_path)],
                                capture_output=True, text=True, timeout=300)
        (folder / 'epubcheck.log').write_text(result.stdout + result.stderr)
        if not check_path.exists():
            raise ValueError('EPUBCheck did not produce a conformance report')
    check = json.loads(check_path.read_text())
    failures = [message for message in check['messages'] if message['severity'] in ('ERROR', 'FATAL')]
    if failures:
        if source_failures(args, group, folder, failures):
            return {'state': 'skipped', 'title': group['title'], 'reason': 'source_epub_validation_errors',
                    'folder': str(folder), 'errors': failures, 'completed_at': now()}
        raise ValueError('EPUBCheck rejected the EPUB: ' + '; '.join(m['ID'] + ': ' + m['message'] for m in failures[:4]))
    warnings = [message for message in check['messages'] if message['severity'] == 'WARNING']
    if warnings:
        return {'state': 'skipped', 'title': group['title'], 'reason': 'epub_validation_warnings',
                'folder': str(folder), 'warnings': warnings, 'completed_at': now()}
    replacement = existing_generated(args.catalog, hashes)
    # Importer performs its own report and input verification and handles an
    # already active report identity before considering replacement.
    receipt = importer.run(SimpleNamespace(catalog=args.catalog, binary=args.binary, report=report_path,
                                           replace_merged=replacement, backup=args.output / 'curation-before-pass.sqlite3'))
    return {'state': 'imported', 'title': group['title'], 'hash': receipt['hash'], 'volume_count': len(hashes),
            'folder': str(folder), 'warnings': sum(m['severity'] == 'WARNING' for m in check['messages']),
            'added_sections': sum(len(v.get('added_numbered_sections', [])) for v in report['volumes']),
            'modified': report['modified'], 'completed_at': now(), 'receipt': receipt}


def run(args):
    args.output.mkdir(parents=True, exist_ok=True)
    lock = (args.output / 'merge-pass.lock').open('a+b')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    selection = json.loads(args.selection.read_text())
    groups = [group for group in selection['groups'] if group['status'] == 'eligible']
    progress_path = args.output / 'progress.json'
    state = json.loads(progress_path.read_text()) if progress_path.exists() else {
        'started_at': now(), 'selected': len(groups), 'outcomes': {}, 'excluded_groups': dict(Counter(g['status'] for g in selection['groups'] if g['status'] != 'eligible'))}
    state['state'] = 'running'
    write_json(progress_path, state)
    merger = load_module('volume_pass_merger', 'merge-epubs.py')
    importer = load_module('volume_pass_importer', 'import-merged-epub.py')
    for group in groups:
        identity = hashlib.sha256(json.dumps([group['title'], [v['hash'] for v in group['volumes']]]).encode()).hexdigest()
        if state['outcomes'].get(identity, {}).get('state') in ('imported', 'skipped'):
            continue
        state['current_title'] = group['title']
        write_json(progress_path, state)
        print('Merging ' + group['title'], flush=True)
        try:
            outcome = one_group(args, group, merger, importer)
        except Exception as error:
            outcome = {'state': 'failed', 'title': group['title'], 'error': str(error), 'completed_at': now()}
        state['outcomes'][identity] = outcome
        state['imported'] = sum(o['state'] == 'imported' for o in state['outcomes'].values())
        state['failed'] = sum(o['state'] == 'failed' for o in state['outcomes'].values())
        state['skipped'] = sum(o['state'] == 'skipped' for o in state['outcomes'].values())
        state['processed'] = state['imported'] + state['failed'] + state['skipped']
        state['updated_at'] = now()
        write_json(progress_path, state)
        print(f"{state['processed']}/{len(groups)}: {outcome['state']} — {group['title']}" + (': ' + outcome['error'] if 'error' in outcome else ''), flush=True)
    if state.get('imported'):
        state['state'] = 'publishing'
        state.pop('current_title', None)
        write_json(progress_path, state)
        publish(args)
    state['state'] = 'completed_with_failures' if state.get('failed') else ('completed_with_skips' if state.get('skipped') else 'completed')
    state['finished_at'] = now()
    state['source_volumes_merged'] = sum(o.get('volume_count', 0) for o in state['outcomes'].values() if o['state'] == 'imported')
    write_json(progress_path, state)
    print(json.dumps({k: state[k] for k in ['state', 'selected', 'imported', 'skipped', 'failed', 'source_volumes_merged'] if k in state}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selection', type=Path, required=True)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--epubcheck', type=Path, required=True)
    args = parser.parse_args()
    for field in ['selection', 'catalog', 'output', 'binary', 'epubcheck']:
        setattr(args, field, getattr(args, field).resolve())
    run(args)


if __name__ == '__main__':
    main()
