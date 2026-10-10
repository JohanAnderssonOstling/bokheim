#!/usr/bin/env python3
"""One-time conversion of already normalized LC/LT caches; never reads API keys."""
import argparse
import json
import os
from pathlib import Path
import re
import sqlite3


def blob(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode('utf-8')


def table(db, name):
    return db.execute("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)", (name,)).fetchone()[0]


def migrate(loc, librarything, output):
    output = Path(output)
    temporary = output.with_name(output.name + '.building')
    if output.exists():
        raise ValueError('output exists; choose a new provider file')
    os.close(os.open(temporary, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600))
    db = None
    try:
        # Use the actual file contract rather than maintaining another schema.
        schema = (Path(__file__).resolve().parents[1] / 'src/schema.rs').read_text()
        sql = re.search(r'const PROVIDERS: &str = "(.*?)";', schema, re.S).group(1)
        db = sqlite3.connect(temporary)
        db.executescript(sql)
        rows = 0
        for provider, path in [('loc', loc), ('librarything', librarything)]:
            if path is None:
                continue
            source = sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)
            try:
                source.execute('BEGIN')
                name = 'loc_api_result' if provider == 'loc' else 'librarything_isbn_lcc'
                for isbn, raw, checked in source.execute(f'SELECT isbn13,result,checked_at FROM {name} ORDER BY isbn13'):
                    value = json.loads(raw)
                    if provider == 'loc':
                        if isinstance(value, dict) and 'Matched' in value:
                            status = 'matched'
                            codes = value['Matched']['classifications']
                            subjects = value['Matched']['subjects']
                        elif value in ('NoMatch', 'Ambiguous'):
                            status = 'no_match' if value == 'NoMatch' else 'ambiguous'
                            codes, subjects = [], []
                        else:
                            raise ValueError(f'unknown LC cache status for {isbn}')
                    else:
                        status = 'matched' if value['work_id'] else 'no_match'
                        codes, subjects = [], []
                        for key, scheme in [('codes', 'library_of_congress'), ('ddc', 'dewey_decimal'), ('bisac', 'bisac')]:
                            codes.extend({
                                'scheme': scheme, 'notation': notation, 'source': 'work',
                                'evidence': [{'method': f"librarything_isbn:{value['work_id']}", 'isbn13': str(isbn),
                                              'open_library_work_id': '', 'open_library_edition_id': ''}],
                            } for notation in value.get(key, []))
                    db.execute('INSERT INTO provider_result VALUES(?,?,?,?,?,?)',
                               (provider, int(isbn), checked * 1000, status, blob(codes), blob(subjects)))
                    rows += 1
                if provider == 'librarything' and table(source, 'librarything_requests'):
                    quota = source.execute('SELECT day,count,last_at FROM librarything_requests WHERE id=1').fetchone()
                    if quota:
                        day, used, last = quota
                        db.execute("INSERT INTO provider_quota VALUES('librarything',?,?)", (day, used))
                        db.execute("INSERT INTO provider_rate VALUES('librarything',?)", (last * 1000 + 2000,))
                if provider == 'loc' and table(source, 'loc_api_rate'):
                    rate = source.execute('SELECT next_ms FROM loc_api_rate WHERE id=1').fetchone()
                    if rate:
                        db.execute("INSERT INTO provider_rate VALUES('loc',?)", rate)
            finally:
                source.close()
        db.commit()
        db.execute('PRAGMA wal_checkpoint(TRUNCATE)')
        db.execute('PRAGMA journal_mode=DELETE')
        db.close()
        db = None
        os.link(temporary, output)  # publish without replacing an existing file
        return rows
    finally:
        if db is not None:
            db.close()
        temporary.unlink(missing_ok=True)
        Path(str(temporary) + '-wal').unlink(missing_ok=True)
        Path(str(temporary) + '-shm').unlink(missing_ok=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--loc', type=Path)
    parser.add_argument('--librarything', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if not args.loc and not args.librarything:
        parser.error('provide at least one cache')
    print(f'migrated {migrate(args.loc, args.librarything, args.output)} completed provider results')
