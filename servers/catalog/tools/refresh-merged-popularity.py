#!/usr/bin/env python3
"""Retain fetched Gutenberg component counts before publishing their raw means.

OBSERVATIONS maps Gutenberg IDs to counts, canonical page URLs, lookup dates,
and retained page hashes. SELECTION lists reviewed combined book hashes.
Keep the observations and downloaded pages alongside the curation audit.
"""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import sqlite3


def refresh(metadata, observations):
    updated = deepcopy(metadata)
    counts = []
    for component in updated['components']:
        entries = [entry for entry in component['source_entries'] if entry['source'] == 'gutenberg.org']
        if not entries:
            continue
        if len(entries) != 1:
            raise ValueError('Review components with multiple Gutenberg editions before refreshing')
        entry = entries[0]
        observation = observations[entry['entry_id']]
        count = observation['downloads']
        if type(count) is not int or not 0 <= count <= 2**64 - 1:
            raise ValueError('Downloads must be an unsigned integer count')
        canonical = f"https://www.gutenberg.org/ebooks/{entry['entry_id']}"
        if observation['listing_url'] != canonical or not observation['observed_at'] or len(observation['page_sha256']) != 64:
            raise ValueError('Missing canonical page, observation date, or page fingerprint')
        evidence = entry['metadata']
        history = evidence.setdefault('download_observations', [])
        if 'downloads' in evidence:
            prior = {key: evidence[key] for key in ('downloads', 'scope', 'listing_url', 'observed_at', 'download_metadata') if key in evidence}
            if not any(all(item.get(key) == value for key, value in prior.items()) for item in history):
                history.append(prior)
        if observation not in history:
            history.append(deepcopy(observation))
        evidence.update({key: observation[key] for key in ('downloads', 'scope', 'listing_url', 'observed_at', 'download_metadata')})
        counts.append(count)
    if not counts:
        raise ValueError('Selected combined book has no Gutenberg components')
    return updated, sum(counts) / len(counts), len(counts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--observations', type=Path, required=True)
    parser.add_argument('--selection', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    selected = {row['hash'] for row in json.loads(args.selection.read_text())}
    observations = json.loads(args.observations.read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    with sqlite3.connect(args.catalog / 'curation.sqlite3', timeout=45) as db:
        changes = []
        for identity, book_hash, raw in db.execute("SELECT entry_id,book_hash,metadata FROM source_entry WHERE source='bokheim:combined-volumes'"):
            if book_hash not in selected:
                continue
            metadata, mean, count = refresh(json.loads(raw), observations)
            changes.append({'entry_id': identity, 'hash': book_hash, 'previous_metadata': raw,
                            'metadata': metadata, 'raw_mean': mean, 'gutenberg_components': count})
        if not changes:
            raise ValueError('No selected combined books exist in this curation database')
        with sqlite3.connect(args.output / 'curation-before.sqlite3') as backup:
            db.backup(backup)
        db.execute('BEGIN IMMEDIATE')
        for row in changes:
            count = db.execute("UPDATE source_entry SET metadata=? WHERE source='bokheim:combined-volumes' AND entry_id=? AND book_hash=? AND metadata=?",
                               (json.dumps(row['metadata'], ensure_ascii=False), row['entry_id'], row['hash'], row['previous_metadata'])).rowcount
            if count != 1:
                raise ValueError('Curation changed while preparing the refresh; retry against current records')
        if db.execute('PRAGMA integrity_check').fetchall() != [('ok',)] or db.execute('PRAGMA foreign_key_check').fetchall():
            raise ValueError('Curation integrity check failed')
        db.commit()
    receipt = {'policy': 'arithmetic_mean_of_component_raw_downloads_before_source_percentiles',
               'updated': len(changes), 'changes': changes,
               'observations_sha256': hashlib.sha256(args.observations.read_bytes()).hexdigest()}
    (args.output / 'refresh.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'updated': len(changes), 'output': str(args.output)}))


if __name__ == '__main__':
    main()
