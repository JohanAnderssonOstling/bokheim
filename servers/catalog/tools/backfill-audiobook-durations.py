#!/usr/bin/env python3
"""Backfill catalog audio durations from the exact Internet Archive recording.

Fetches only metadata, retains HTTP evidence, and writes staged databases.
The input curation database and installed snapshot are never edited.
"""
import argparse
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sqlite3
import time
from urllib.parse import unquote, urlencode, urlsplit
from urllib.request import Request, urlopen


MAX_DURATION = 2**63 - 1


def now():
    return datetime.now(timezone.utc).isoformat()


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + '.partial')
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    temporary.replace(path)


def archive_item(url):
    parsed = urlsplit(url or '')
    if parsed.scheme not in ('http', 'https') or parsed.hostname not in ('archive.org', 'www.archive.org'):
        return None
    if parsed.username or parsed.password:
        return None
    parts = [unquote(part) for part in parsed.path.split('/') if part]
    if len(parts) < 2 or parts[0] not in ('download', 'compress', 'details'):
        return None
    item = parts[1]
    return item if re.fullmatch(r'[A-Za-z0-9_.-]+', item) else None



def archive_asset(url):
    item = archive_item(url)
    if item is None:
        return None
    parts = [unquote(part) for part in urlsplit(url).path.split('/') if part]
    return item, '/'.join(parts[2:])


def clock_from_ms(milliseconds):
    seconds = milliseconds // 1000
    return f'{seconds // 3600}:{seconds // 60 % 60:02d}:{seconds % 60:02d}'


def librivox_duration(book):
    seconds = book.get('totaltimesecs')
    if not isinstance(seconds, int) or isinstance(seconds, bool) or not 0 < seconds <= MAX_DURATION // 1000:
        return None
    clock = book.get('totaltime')
    if isinstance(clock, str) and re.fullmatch(r'\d+:\d{2}', clock):
        minutes, remainder = map(int, clock.split(':'))
        clock_ms = (minutes * 60 + remainder) * 1000 if remainder < 60 else None
    else:
        clock_ms = duration_ms(clock)
    return seconds * 1000 if clock_ms == seconds * 1000 else None


def duration_ms(value):
    # Archive's recording runtime is a clock, not an unlabelled number.
    if not isinstance(value, str) or not re.fullmatch(r'\d+:\d{2}:\d{2}(?:\.\d+)?', value.strip()):
        return None
    hours, minutes, seconds = value.strip().split(':')
    try:
        seconds = Decimal(seconds)
        if int(minutes) >= 60 or seconds >= 60:
            return None
        milliseconds = int((int(hours) * 3600 + int(minutes) * 60 + seconds) * 1000)
        return milliseconds if 0 < milliseconds <= MAX_DURATION else None
    except (InvalidOperation, ValueError, OverflowError):
        return None


def fetch_batch(items, output, index):
    url = 'https://archive.org/advancedsearch.php?' + urlencode({
        'q': 'identifier:(' + ' OR '.join(f'"{item}"' for item in items) + ')',
        'fl[]': ['identifier', 'runtime'], 'rows': len(items), 'output': 'json',
    }, doseq=True)
    path = output / f'archive-{index:03d}.json'
    receipt_path = path.with_suffix('.http.json')
    if path.exists() and receipt_path.exists():
        raw = path.read_bytes()
        receipt = json.loads(receipt_path.read_text())
        if receipt['url'] != url or receipt['sha256'] != hashlib.sha256(raw).hexdigest():
            raise ValueError(f'Cached source evidence differs: {path}')
    else:
        for attempt in range(3):
            try:
                time.sleep(0.5)
                with urlopen(Request(url, headers={'User-Agent': 'bokheim-catalog-duration-backfill/1.0'}), timeout=60) as response:
                    raw = response.read()
                    receipt = {'url': url, 'response_url': response.url, 'observed_at': now(),
                               'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}
                parsed = json.loads(raw)
                if parsed.get('responseHeader', {}).get('status') != 0 or 'response' not in parsed:
                    raise ValueError(f'Archive search failed: {str(parsed)[:300]}')
                path.write_bytes(raw)
                write_json(receipt_path, receipt)
                break
            except Exception:
                if attempt == 2:
                    raise
                time.sleep(2 ** (attempt + 1))
    response = json.loads(raw)['response']
    if response['numFound'] > len(items):
        raise ValueError('Archive query was not restricted to the selected recordings')
    rows = {}
    for row in response['docs']:
        item = row['identifier']
        if item not in items or item in rows:
            raise ValueError('Archive returned an unexpected or duplicate recording')
        rows[item] = (row, {'provider': 'archive.org', 'archive_identifier': item,
                           'recording_url': f'https://archive.org/details/{item}',
                           'source_url': url, 'observed_at': receipt['observed_at'],
                           'response_sha256': receipt['sha256'], 'response_file': str(path),
                           'runtime': row.get('runtime')})
    return rows



def fetch_librivox_fallback(targets, observations, output):
    pending = {row['archive_identifier'] for row in make_plan(targets, observations)['missing']
               if row['archive_identifier'] is not None}
    if not pending:
        return
    wanted = {item for target in targets if (item := archive_item(target['download_url'])) is not None}
    matches = {item: [] for item in wanted}
    for offset in range(0, 100_000, 500):
        url = 'https://librivox.org/api/feed/audiobooks/?' + urlencode({
            'format': 'json', 'limit': 500, 'offset': offset,
            'fields': 'id,title,url_zip_file,totaltime,totaltimesecs,url_librivox',
        })
        path = output / f'librivox-{offset:05d}.json'
        receipt_path = path.with_suffix('.http.json')
        if path.exists() and receipt_path.exists():
            raw = path.read_bytes()
            receipt = json.loads(receipt_path.read_text())
            if receipt['url'] != url or receipt['sha256'] != hashlib.sha256(raw).hexdigest():
                raise ValueError(f'Cached LibriVox evidence differs: {path}')
        else:
            for attempt in range(3):
                try:
                    # LibriVox requests a pause of several seconds between pages.
                    time.sleep(3)
                    with urlopen(Request(url, headers={'User-Agent': 'bokheim-catalog-duration-backfill/1.0'}), timeout=60) as response:
                        raw = response.read()
                        receipt = {'url': url, 'response_url': response.url, 'observed_at': now(),
                                   'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}
                    if 'books' not in json.loads(raw):
                        raise ValueError('LibriVox did not return an audiobook page')
                    path.write_bytes(raw)
                    write_json(receipt_path, receipt)
                    break
                except Exception:
                    if attempt == 2:
                        raise
                    time.sleep(2 ** (attempt + 1))
        books = json.loads(raw)['books']
        for book in books:
            item = archive_item(book.get('url_zip_file'))
            if item not in matches:
                continue
            duration = librivox_duration(book)
            if duration is None:
                continue
            matches[item].append(({'runtime': clock_from_ms(duration)}, {
                'provider': 'librivox.org', 'archive_identifier': item,
                'recording_url': book.get('url_librivox'), 'download_url': book['url_zip_file'],
                'librivox_id': book['id'], 'runtime': book['totaltime'], 'total_seconds': book['totaltimesecs'],
                'source_url': url, 'observed_at': receipt['observed_at'],
                'response_file': str(path), 'response_sha256': receipt['sha256'],
            }))
        matched = sum(bool(rows) for rows in matches.values())
        print(f'Verified LibriVox metadata through record {offset + len(books)}; matched {matched}/{len(wanted)} recordings', flush=True)
        if len(books) < 500:
            break
    else:
        raise ValueError('LibriVox pagination did not terminate')
    for item, rows in matches.items():
        if rows and len({duration_ms(row[0]['runtime']) for row in rows}) == 1:
            observations[item] = rows[0]



class SourcePage(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
        self.text = []

    def handle_starttag(self, tag, attrs):
        if tag == 'a':
            self.links.extend(value for key, value in attrs if key == 'href' and value)

    def handle_data(self, data):
        self.text.append(data)


def retained_source_pages(targets, observations, directory):
    for path in directory.glob('loyalbooks-*.html'):
        receipt = json.loads(path.with_suffix('.http.json').read_text())
        raw = path.read_bytes()
        if receipt['sha256'] != hashlib.sha256(raw).hexdigest():
            raise ValueError('Retained source page hash differs')
        if urlsplit(receipt['url']).hostname not in ('loyalbooks.com', 'www.loyalbooks.com'):
            raise ValueError('Expected a Loyal Books source page')
        page = SourcePage()
        page.feed(raw.decode('utf-8'))
        clocks = {clock for clock in re.findall(r'\((\d+:\d{2}:\d{2})\s+long\)', ' '.join(page.text)) if duration_ms(clock)}
        if len(clocks) != 1:
            continue
        clock = clocks.pop()
        for target in targets:
            if target.get('entry_url') != receipt['url']:
                continue
            asset = archive_asset(target['download_url'])
            if asset is None or not any(archive_asset(link) == asset for link in page.links):
                continue
            observations[asset[0]] = ({'runtime': clock}, {
                'provider': 'loyalbooks.com', 'archive_identifier': asset[0], 'runtime': clock,
                'source_url': receipt['url'], 'download_url': target['download_url'],
                'observed_at': receipt['observed_at'], 'response_sha256': receipt['sha256'],
                'response_file': str(path),
            })


def select_targets(db):
    return [dict(zip(('key', 'source', 'entry_id', 'title', 'download_url', 'metadata', 'entry_url'), row)) for row in db.execute(
        "SELECT b.entry_key,s.source,s.entry_id,b.title,b.file_url,s.metadata,b.entry_url "
        "FROM external_book b JOIN source_entry s ON s.external_entry_key=b.entry_key "
        "WHERE b.format IN(8,16) ORDER BY b.entry_key,s.source,s.entry_id")]


def make_plan(targets, observations):
    updates, missing, known = [], [], []
    for target in targets:
        metadata = json.loads(target['metadata'])
        current = metadata.get('audiobook_duration_ms')
        if isinstance(current, int) and not isinstance(current, bool) and 0 < current <= MAX_DURATION:
            known.append(target['key'])
            continue
        item = archive_item(target['download_url'])
        # Source evidence must name the same recording as the current download.
        evidence_item = archive_item(metadata.get('audiobook_download_url'))
        found = observations.get(item) if item is not None and item == evidence_item and archive_asset(target['download_url']) == archive_asset(metadata.get('audiobook_download_url')) else None
        duration = duration_ms(found[0].get('runtime')) if found else None
        identity = {key: target[key] for key in ('key', 'source', 'entry_id', 'title', 'download_url')}
        if duration is None:
            missing.append({**identity, 'archive_identifier': item,
                            'reason': 'no unambiguous runtime for the retained recording',
                            'runtime': found[0].get('runtime') if found else None})
            continue
        updates.append({**identity, 'duration_ms': duration, 'evidence': found[1]})
    return {'created_at': now(), 'targets': len(targets), 'already_known': known,
            'updated_recordings': len({row['key'] for row in updates}),
            'updates': updates, 'missing': missing}


def backup(source, destination):
    if destination.exists():
        raise ValueError(f'Refusing to replace {destination}')
    with sqlite3.connect(f'file:{source}?mode=ro', uri=True) as original, sqlite3.connect(destination) as result:
        original.backup(result)


def verify(db):
    if db.execute('PRAGMA integrity_check').fetchone() != ('ok',) or db.execute('PRAGMA foreign_key_check').fetchall():
        raise ValueError('Database integrity verification failed')


def stage_curation(source, destination, plan):
    backup(source, destination)
    with sqlite3.connect(destination) as db:
        db.execute('PRAGMA foreign_keys=ON')
        for update in plan['updates']:
            row = db.execute('SELECT metadata FROM source_entry WHERE source=? AND entry_id=? AND external_entry_key=?',
                             (update['source'], update['entry_id'], update['key'])).fetchone()
            if row is None:
                raise ValueError('A selected source entry is no longer present')
            metadata = json.loads(row[0])
            if archive_item(metadata.get('audiobook_download_url')) != update['evidence']['archive_identifier']:
                raise ValueError('The source recording changed after planning')
            metadata['audiobook_duration_ms'] = update['duration_ms']
            metadata['audiobook_duration_evidence'] = update['evidence']
            db.execute('UPDATE source_entry SET metadata=? WHERE source=? AND entry_id=?',
                       (json.dumps(metadata, ensure_ascii=False, separators=(',', ':')), update['source'], update['entry_id']))
        verify(db)


def stage_snapshot(source, destination, plan, release):
    backup(source, destination)
    with sqlite3.connect(destination) as db:
        db.execute('PRAGMA foreign_keys=ON')
        old_format, old_release = db.execute('SELECT format_version,release_id FROM snapshot WHERE singleton=1').fetchone()
        if old_format != 8 or release <= old_release:
            raise ValueError('Expected a format-8 snapshot and a strictly newer release')
        db.execute('CREATE TABLE IF NOT EXISTS book_audiobook_duration('
                   'book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id),'
                   'duration_ms INTEGER NOT NULL CHECK(duration_ms>0)) STRICT')
        for update in plan['updates']:
            row = db.execute('SELECT row_id,download_url,format FROM book WHERE key=? AND external=1',
                             (bytes.fromhex(update['key']),)).fetchone()
            if row is None or row[2] not in (8, 16) or archive_asset(row[1]) != archive_asset(update['download_url']):
                raise ValueError(f"Installed snapshot differs from selected recording: {update['key']}")
            previous = db.execute('SELECT duration_ms FROM book_audiobook_duration WHERE book_row_id=?', (row[0],)).fetchone()
            if previous and previous[0] != update['duration_ms']:
                raise ValueError('Refusing to replace an existing duration')
            db.execute('INSERT OR IGNORE INTO book_audiobook_duration VALUES(?,?)', (row[0], update['duration_ms']))
        # Browse caches are built from source tables by the application.
        for kind in ('view', 'index', 'table'):
            names = db.execute("SELECT name FROM sqlite_schema WHERE type=? AND name GLOB 'catalog_*'", (kind,)).fetchall()
            for (name,) in names:
                db.execute(f'DROP {kind.upper()} "{name}"')
        db.execute('UPDATE snapshot SET release_id=? WHERE singleton=1', (release,))
        verify(db)
    return {'release_id': release, 'format_version': old_format, 'previous_release_id': old_release}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog-db', type=Path, required=True)
    parser.add_argument('--snapshot', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--apply', action='store_true', help='Write staged copies of both databases')
    parser.add_argument('--release-id', type=int)
    args = parser.parse_args()
    if args.apply and args.release_id is None:
        parser.error('--apply requires --release-id')
    args.output.mkdir(parents=True, exist_ok=True)
    evidence = args.output / 'evidence'
    evidence.mkdir(exist_ok=True)
    with sqlite3.connect(f'file:{args.catalog_db}?mode=ro', uri=True) as db:
        targets = select_targets(db)
    items = sorted({item for row in targets if (item := archive_item(row['download_url']))})
    observations = {}
    for index, start in enumerate(range(0, len(items), 20)):
        observations.update(fetch_batch(items[start:start + 20], evidence, index))
        print(f"Retrieved runtime metadata for {min(start + 20, len(items))}/{len(items)} recordings", flush=True)
    fetch_librivox_fallback(targets, observations, evidence)
    retained_source_pages(targets, observations, evidence)
    plan = make_plan(targets, observations)
    write_json(args.output / 'plan.json', plan)
    if args.apply:
        stage_curation(args.catalog_db, args.output / 'curation-after.sqlite3', plan)
        release = stage_snapshot(args.snapshot, args.output / 'catalog-after.sqlite3', plan, args.release_id)
        write_json(args.output / 'result.json', {**release, 'recordings_updated': plan['updated_recordings'],
                                               'missing': len(plan['missing']), 'verified_at': now()})
    print(json.dumps({'recordings_updated': plan['updated_recordings'], 'missing': len(plan['missing']),
                      'already_known': len(plan['already_known'])}), flush=True)


if __name__ == '__main__':
    main()
