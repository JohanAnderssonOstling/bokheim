#!/usr/bin/env python3
import sqlite3
import sys

target = sys.argv[1]
batch_size = 100_000
db = sqlite3.connect(target, timeout=120)
db.execute("PRAGMA busy_timeout=120000")

last = 0
total = 0
while True:
    row = db.execute(
        "SELECT max(edition_id) FROM (SELECT edition_id FROM edition_classification "
        "WHERE edition_id>? ORDER BY edition_id LIMIT ?)", (last, batch_size)
    ).fetchone()
    end = row[0]
    if end is None:
        break
    db.execute("BEGIN IMMEDIATE")
    try:
        db.execute("""
            INSERT OR IGNORE INTO title_author_classification(
                normalized_title, normalized_subtitle, normalized_author,
                publication_year, normalized_publisher, scheme, notation
            )
            SELECT DISTINCT
                bibliography.normalized_title,
                lower(trim(COALESCE(subtitle.subtitle,''))),
                lower(trim(COALESCE(author.name,''))),
                COALESCE(bibliography.publication_year,0),
                COALESCE(publisher.normalized_name,''),
                classification.scheme, classification.notation
            FROM edition_classification classification
            JOIN edition_bibliography bibliography USING(edition_id)
            JOIN edition_author edition_author USING(edition_id)
            JOIN author ON author.author_id=edition_author.author_id
            LEFT JOIN edition_subtitle subtitle USING(edition_id)
            LEFT JOIN edition_publisher publisher
              ON publisher.edition_id=classification.edition_id AND publisher.position=1
            WHERE classification.edition_id>? AND classification.edition_id<=?
              AND trim(COALESCE(author.name,''))!=''
        """, (last, end))
        added = db.execute("SELECT changes()").fetchone()[0]
        db.commit()
    except Exception:
        db.rollback()
        raise
    total += added
    last = end
    print({"through_edition_id": last, "added": added, "total": total}, flush=True)

db.execute("ANALYZE title_author_classification")
print({"status": "complete", "total_added": total}, flush=True)
