#!/usr/bin/env python3
"""Retain Gutenberg subjects for explicit catalog targets using cached source pages."""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import importlib.util
import json
from pathlib import Path
import sqlite3
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--targets', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--workers', type=int, default=3)
    parser.add_argument('--delay', type=float, default=1)
    parser.add_argument('--fetch-only', action='store_true')
    args = parser.parse_args()
    if args.workers < 1 or args.delay < 0:
        parser.error('workers must be positive and delay nonnegative')
    args.output.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location('gutenberg_ingestion', Path(__file__).with_name('ingest-gutenberg.py'))
    ingestion = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(ingestion)
    selection = json.loads(args.targets.read_text())
    grouped = {}
    for target in selection['entries']:
        table, key = target['key'].split(':', 1)
        if table not in ('book', 'external_book') or len(target['gutenberg_ids']) != 1:
            raise ValueError('Each target must identify one catalog record and one Gutenberg book')
        number = target['gutenberg_ids'][0]
        if not number.isdigit() or int(number) <= 0:
            raise ValueError('Invalid Gutenberg identifier')
        grouped.setdefault(number, []).append({'table': table, 'key': key})
    ingestion.write_json(args.output / 'selection.json', selection)
    fetcher = ingestion.Fetcher(args.delay)
    failures = {}
    # A failed receipt write must not make an unverified page look resumable.
    for number in grouped:
        page = args.output / 'pages' / f'{number}.html'
        if page.exists() and not page.with_suffix('.html.http.json').exists():
            page.replace(page.with_suffix('.html.unreceipted'))
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        jobs = {pool.submit(fetcher.fetch, f'https://www.gutenberg.org/ebooks/{number}', args.output / 'pages' / f'{number}.html'): number for number in grouped}
        for done, future in enumerate(as_completed(jobs), 1):
            number = jobs[future]
            try:
                future.result()
            except Exception as error:
                failures[number] = str(error)
            ingestion.write_json(args.output / 'fetch-progress.json', {'processed': done, 'pages': len(grouped), 'failures': failures})
            if done % 10 == 0 or done == len(grouped):
                print(f'Fetched {done}/{len(grouped)} source pages; failures={len(failures)}', flush=True)
    if failures:
        raise RuntimeError('Source retrieval incomplete; rerun to resume cached pages')
    manifest = []
    for number, targets in grouped.items():
        page = args.output / 'pages' / f'{number}.html'
        receipt = json.loads(page.with_suffix('.html.http.json').read_text())
        manifest.append({'page_url': f'https://www.gutenberg.org/ebooks/{number}', 'page_file': f'pages/{number}.html', 'observed_at': receipt['observed_at'], 'targets': targets})
    ingestion.write_json(args.output / 'manifest.json', manifest)
    if args.fetch_only:
        return
    backup = args.output / 'curation-before.sqlite3'
    if not backup.exists():
        with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db, sqlite3.connect(backup) as destination:
            db.backup(destination)
    result = subprocess.run([str(args.binary), str(args.catalog), 'backfill-gutenberg-subjects', str(args.output / 'manifest.json')], capture_output=True, text=True, check=True)
    ingestion.write_json(args.output / 'source-subject-outcomes.json', json.loads(result.stdout))
    print('Source subjects retained; publish a catalog snapshot after enrichment and review.', flush=True)


if __name__ == '__main__':
    main()
