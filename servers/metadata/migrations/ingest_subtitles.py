#!/usr/bin/env python3
import sqlite3
import sys

source, target = sys.argv[1:3]
batch_size = 100_000
db = sqlite3.connect(target, timeout=120)
db.execute("PRAGMA busy_timeout=120000")
db.execute("ATTACH DATABASE ? AS subtitle_source", (source,))

source_snapshot = db.execute("SELECT schema_version,dump_date FROM subtitle_source.snapshot WHERE singleton=1").fetchone()
target_snapshot = db.execute("SELECT schema_version,dump_date FROM main.snapshot WHERE singleton=1").fetchone()
if source_snapshot != target_snapshot:
    raise SystemExit(f"snapshot mismatch: source={source_snapshot} target={target_snapshot}")

db.executescript("""
CREATE TABLE IF NOT EXISTS edition_subtitle(
    edition_id INTEGER PRIMARY KEY,
    subtitle TEXT NOT NULL,
    normalized_title TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS work_subtitle(
    work_id INTEGER PRIMARY KEY,
    subtitle TEXT NOT NULL,
    normalized_title TEXT NOT NULL
);
""")

total = 0
for table, key in (("edition_subtitle", "edition_id"), ("work_subtitle", "work_id")):
    last = 0
    while True:
        end = db.execute(
            f"SELECT max({key}) FROM (SELECT {key} FROM subtitle_source.{table} WHERE {key}>? ORDER BY {key} LIMIT ?)",
            (last, batch_size),
        ).fetchone()[0]
        if end is None:
            break
        db.execute("BEGIN IMMEDIATE")
        try:
            db.execute(
                f"INSERT OR REPLACE INTO main.{table}({key},subtitle,normalized_title) "
                f"SELECT {key},subtitle,normalized_title FROM subtitle_source.{table} WHERE {key}>? AND {key}<=?",
                (last, end),
            )
            copied = db.execute("SELECT changes()").fetchone()[0]
            db.commit()
        except Exception:
            db.rollback()
            raise
        total += copied
        last = end
        print({"table": table, "through": last, "copied": copied, "total": total}, flush=True)

db.executescript("""
CREATE INDEX IF NOT EXISTS edition_subtitle_by_title ON edition_subtitle(normalized_title,edition_id);
CREATE INDEX IF NOT EXISTS work_subtitle_by_title ON work_subtitle(normalized_title,work_id);
ANALYZE edition_subtitle;
ANALYZE work_subtitle;
""")
print({"status": "complete", "total": total}, flush=True)
