"""Apply reviewed, versioned supplements to the development LCC reference only."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_UPDATES = ROOT / "shared/subject-projection/data/lcc-reference-updates"
DEFAULT_DATABASE = ROOT / "shared/subject-projection/data/lcc-mdsconnect-2016-baseline.sqlite3"


def validate_reference(connection: sqlite3.Connection) -> None:
    tables = {r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if "concept" in tables or not {"metadata", "classification_record", "classification_span", "caption_hierarchy"} <= tables:
        raise ValueError("Expected the development LCC reference database, not a curated taxonomy")
    release = connection.execute("SELECT value FROM metadata WHERE key='source_release'").fetchone()
    if release != ("MDSConnect Classification 2016 retrospective",):
        raise ValueError("This supplement requires the reviewed MDSConnect 2016 baseline")


def exact_record(connection: sqlite3.Connection, code: str) -> int:
    rows = connection.execute(
        "SELECT record_id FROM classification_span WHERE table_id IS NULL AND start_number=? AND end_number IS NULL",
        (code,),
    ).fetchall()
    if len(rows) != 1:
        raise ValueError(f"Expected one exact reference record for {code}, found {len(rows)}")
    return rows[0][0]


def apply_updates(connection: sqlite3.Connection, directory: Path = DEFAULT_UPDATES) -> dict[str, int]:
    """Apply all manifests atomically. Reapplying unchanged manifests is a no-op."""
    validate_reference(connection)
    paths = sorted(directory.glob("*.json"))
    if not paths:
        raise ValueError(f"No reference update manifests in {directory}")
    counts = dict(additions=0, caption_updates=0, redirects=0, notes=0)
    connection.execute("SAVEPOINT lcc_reference_updates")
    try:
        connection.execute("""CREATE TABLE IF NOT EXISTS reference_update_provenance (
            patch_id TEXT NOT NULL, record_id INTEGER NOT NULL REFERENCES classification_record(record_id),
            source_url TEXT NOT NULL, original_caption TEXT, PRIMARY KEY(patch_id, record_id)
        ) STRICT""")
        connection.execute("""CREATE TABLE IF NOT EXISTS reference_redirect (
            code TEXT PRIMARY KEY, target_code TEXT NOT NULL, patch_id TEXT NOT NULL, source_url TEXT NOT NULL
        ) STRICT""")
        connection.execute("""CREATE TABLE IF NOT EXISTS reference_note (
            code TEXT NOT NULL, note TEXT NOT NULL, patch_id TEXT NOT NULL, source_url TEXT NOT NULL,
            PRIMARY KEY(code, patch_id, note)
        ) STRICT""")
        for path in paths:
            raw = path.read_bytes()
            patch = json.loads(raw)
            patch_id = patch["patch_id"]
            digest = hashlib.sha256(raw).hexdigest()
            key = f"reference_update:{patch_id}"
            applied = connection.execute("SELECT value FROM metadata WHERE key=?", (key,)).fetchone()
            if applied:
                if applied[0] != digest:
                    raise ValueError(f"Applied manifest {patch_id} changed; create a new version")
                continue
            for entry in patch["additions"]:
                if connection.execute(
                    "SELECT 1 FROM classification_span WHERE table_id IS NULL AND start_number=? AND end_number IS ?",
                    (entry["start"], entry["end"]),
                ).fetchone():
                    raise ValueError(f"Addition already exists without this patch: {entry['start']}")
                control = f"local:{patch_id}:{entry['start']}:{entry['end'] or ''}"
                rid = connection.execute(
                    "INSERT INTO classification_record(control_number,record_status,scheme,edition,caption) VALUES(?, 'n', 'lcc', NULL, ?)",
                    (control, entry["caption"]),
                ).lastrowid
                connection.execute("INSERT INTO classification_span VALUES(?,0,NULL,?,?)", (rid, entry["start"], entry["end"]))
                connection.executemany("INSERT INTO caption_hierarchy VALUES(?,?,'caption',?)",
                                       [(rid, i, label) for i, label in enumerate(entry["hierarchy"])])
                connection.execute("INSERT INTO reference_update_provenance VALUES(?,?,?,NULL)", (patch_id, rid, entry["source"]))
                counts["additions"] += 1
            for entry in patch["caption_updates"]:
                rid = exact_record(connection, entry["code"])
                old = connection.execute("SELECT caption FROM classification_record WHERE record_id=?", (rid,)).fetchone()[0]
                if old != entry["old"]:
                    raise ValueError(f"Unexpected old caption for {entry['code']}: {old}")
                connection.execute("UPDATE classification_record SET caption=? WHERE record_id=?", (entry["new"], rid))
                connection.execute("INSERT INTO reference_update_provenance VALUES(?,?,?,?)", (patch_id, rid, entry["source"], old))
                counts["caption_updates"] += 1
            for entry in patch["redirects"]:
                exact_record(connection, entry["code"])
                exact_record(connection, entry["target"])
                connection.execute("INSERT INTO reference_redirect VALUES(?,?,?,?)", (entry["code"], entry["target"], patch_id, entry["source"]))
                counts["redirects"] += 1
            for entry in patch["notes"]:
                exact_record(connection, entry["code"])
                connection.execute("INSERT INTO reference_note VALUES(?,?,?,?)", (entry["code"], entry["note"], patch_id, entry["source"]))
                counts["notes"] += 1
            connection.execute("INSERT INTO metadata VALUES(?,?)", (key, digest))
            connection.execute("INSERT INTO metadata VALUES(?,?)", (f"reference_update_description:{patch_id}", patch["description"]))
        if connection.execute("PRAGMA foreign_key_check").fetchone():
            raise ValueError("LCC reference foreign key check failed")
        connection.execute("RELEASE lcc_reference_updates")
    except BaseException:
        connection.execute("ROLLBACK TO lcc_reference_updates")
        connection.execute("RELEASE lcc_reference_updates")
        raise
    return counts


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path, default=DEFAULT_DATABASE)
    parser.add_argument("--updates", type=Path, default=DEFAULT_UPDATES)
    args = parser.parse_args()
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=rw", uri=True) as connection:
        connection.execute("PRAGMA foreign_keys=ON")
        validate_reference(connection)
        backup = args.database.with_suffix(".before-reference-updates.sqlite3")
        if not backup.exists():
            with sqlite3.connect(backup) as destination:
                connection.backup(destination)
            print(f"Backup: {backup}")
        print(json.dumps(apply_updates(connection, args.updates), sort_keys=True))
        assert connection.execute("PRAGMA integrity_check").fetchone()[0] == "ok"


if __name__ == "__main__":
    main()
