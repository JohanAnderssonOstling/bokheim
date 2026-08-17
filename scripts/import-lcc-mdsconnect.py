#!/usr/bin/env python3
"""Build a local LCC structural database from the free LoC MDSConnect snapshot.

The upstream MARCXML expands to roughly 750 MB, so it is downloaded to a
temporary file and is not checked into the repository.  The generated SQLite
database is also a development input (and is ignored by the repository's
database ignore rule), not an application runtime database.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import os
from pathlib import Path
import shutil
import sqlite3
import sys
import tempfile
import urllib.request
import xml.etree.ElementTree as ET


SOURCE_URL = "https://www.loc.gov/cds/downloads/MDSConnect/Class.2016.part01.xml.gz"
SOURCE_SHA256 = "0df1429947b6e12dd975c8065106dbbd8cce1d61527e95aa4ad9f062a289009f"
MARC = "{http://www.loc.gov/MARC21/slim}"


SCHEMA = """
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE classification_record (
    record_id INTEGER PRIMARY KEY,
    control_number TEXT NOT NULL UNIQUE,
    record_status TEXT NOT NULL,
    scheme TEXT NOT NULL,
    edition TEXT,
    caption TEXT
) STRICT;

CREATE TABLE classification_span (
    record_id INTEGER NOT NULL REFERENCES classification_record(record_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    table_id TEXT,
    start_number TEXT NOT NULL,
    end_number TEXT,
    PRIMARY KEY (record_id, ordinal)
) STRICT;

CREATE TABLE caption_hierarchy (
    record_id INTEGER NOT NULL REFERENCES classification_record(record_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('caption', 'summary')),
    label TEXT NOT NULL,
    PRIMARY KEY (record_id, ordinal)
) STRICT;

CREATE TABLE number_hierarchy (
    record_id INTEGER NOT NULL REFERENCES classification_record(record_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    start_number TEXT NOT NULL,
    end_number TEXT,
    PRIMARY KEY (record_id, ordinal)
) STRICT;

CREATE TABLE table_sequence (
    record_id INTEGER NOT NULL REFERENCES classification_record(record_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    sequence_number TEXT NOT NULL,
    PRIMARY KEY (record_id, ordinal)
) STRICT;
"""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--archive",
        type=Path,
        help="use an existing Class.2016.part01.xml.gz instead of downloading it",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("client/library-model/data/lcc-mdsconnect-2016.sqlite3"),
        help="generated SQLite path (default: %(default)s)",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def obtain_archive(explicit: Path | None, temporary_directory: Path) -> Path:
    if explicit is not None:
        archive = explicit.resolve()
        if not archive.is_file():
            raise SystemExit(f"archive does not exist: {archive}")
    else:
        archive = temporary_directory / "Class.2016.part01.xml.gz"
        print(f"Downloading {SOURCE_URL}", file=sys.stderr)
        request = urllib.request.Request(SOURCE_URL, headers={"User-Agent": "Bokheim taxonomy importer"})
        with urllib.request.urlopen(request) as response, archive.open("wb") as target:
            shutil.copyfileobj(response, target)

    actual_digest = sha256(archive)
    if actual_digest != SOURCE_SHA256:
        raise SystemExit(
            f"unexpected archive SHA-256: {actual_digest}\nexpected: {SOURCE_SHA256}"
        )
    return archive


def subfields(field: ET.Element, code: str) -> list[str]:
    return [
        value
        for child in field.findall(f"{MARC}subfield")
        if child.get("code") == code and (value := " ".join((child.text or "").split()))
    ]


def first(values: list[str]) -> str | None:
    return values[0] if values else None


def import_record(connection: sqlite3.Connection, record: ET.Element) -> bool:
    leader = record.findtext(f"{MARC}leader") or ""
    control_number = None
    for control_field in record.findall(f"{MARC}controlfield"):
        if control_field.get("tag") == "001":
            control_number = " ".join((control_field.text or "").split())
            break

    scheme = None
    edition = None
    classification = None
    for data_field in record.findall(f"{MARC}datafield"):
        tag = data_field.get("tag")
        if tag == "084":
            scheme = first(subfields(data_field, "a"))
            edition = first(subfields(data_field, "c"))
        elif tag == "153":
            classification = data_field

    if not control_number or classification is None or scheme != "lcc":
        return False

    starts = subfields(classification, "a")
    if not starts:
        return False
    ends = subfields(classification, "c")
    tables = subfields(classification, "z")
    captions = subfields(classification, "j")
    cursor = connection.execute(
        "INSERT INTO classification_record(control_number,record_status,scheme,edition,caption) VALUES(?,?,?,?,?)",
        (control_number, leader[5:6] if len(leader) > 5 else "", scheme, edition, first(captions)),
    )
    record_id = cursor.lastrowid

    for ordinal, start in enumerate(starts):
        connection.execute(
            "INSERT INTO classification_span(record_id,ordinal,table_id,start_number,end_number) VALUES(?,?,?,?,?)",
            (
                record_id,
                ordinal,
                tables[ordinal] if ordinal < len(tables) else first(tables),
                start,
                ends[ordinal] if ordinal < len(ends) else (first(ends) if len(starts) == 1 else None),
            ),
        )

    hierarchy_ordinal = 0
    pending_number_starts: list[str] = []
    pending_number_ends: list[str] = []
    sequence_ordinal = 0
    for child in classification.findall(f"{MARC}subfield"):
        code = child.get("code")
        value = " ".join((child.text or "").split())
        if not value:
            continue
        if code in ("h", "k"):
            connection.execute(
                "INSERT INTO caption_hierarchy(record_id,ordinal,kind,label) VALUES(?,?,?,?)",
                (record_id, hierarchy_ordinal, "caption" if code == "h" else "summary", value),
            )
            hierarchy_ordinal += 1
        elif code == "e":
            pending_number_starts.append(value)
        elif code == "f":
            pending_number_ends.append(value)
        elif code == "y":
            connection.execute(
                "INSERT INTO table_sequence(record_id,ordinal,sequence_number) VALUES(?,?,?)",
                (record_id, sequence_ordinal, value),
            )
            sequence_ordinal += 1

    for number_start_ordinal, start in enumerate(pending_number_starts):
        connection.execute(
            "INSERT INTO number_hierarchy(record_id,ordinal,start_number,end_number) VALUES(?,?,?,?)",
            (
                record_id,
                number_start_ordinal,
                start,
                pending_number_ends[number_start_ordinal]
                if number_start_ordinal < len(pending_number_ends)
                else None,
            ),
        )
    return True


def build_database(archive: Path, output: Path) -> tuple[int, int]:
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_output = output.with_name(f".{output.name}.{os.getpid()}.tmp")
    if temporary_output.exists():
        temporary_output.unlink()

    connection = sqlite3.connect(temporary_output)
    try:
        connection.executescript(SCHEMA)
        connection.executemany(
            "INSERT INTO metadata(key,value) VALUES(?,?)",
            [
                ("source_url", SOURCE_URL),
                ("source_sha256", SOURCE_SHA256),
                ("source_release", "MDSConnect Classification 2016 retrospective"),
                ("format_version", "1"),
            ],
        )
        imported = 0
        visited = 0
        with gzip.open(archive, "rb") as xml_stream:
            for _, element in ET.iterparse(xml_stream, events=("end",)):
                if element.tag != f"{MARC}record":
                    continue
                visited += 1
                if import_record(connection, element):
                    imported += 1
                element.clear()
                if visited % 25_000 == 0:
                    print(f"Read {visited:,} records; imported {imported:,} LCC records", file=sys.stderr)
                    connection.commit()

        connection.executescript(
            """
            CREATE INDEX classification_span_start_idx ON classification_span(start_number, end_number);
            CREATE INDEX classification_span_table_idx ON classification_span(table_id, start_number);
            CREATE INDEX caption_hierarchy_label_idx ON caption_hierarchy(label, record_id);
            PRAGMA wal_checkpoint(TRUNCATE);
            """
        )
        connection.commit()
        integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
        if integrity != "ok":
            raise RuntimeError(f"generated database failed integrity check: {integrity}")
    except BaseException:
        connection.close()
        temporary_output.unlink(missing_ok=True)
        raise
    else:
        connection.close()

    os.replace(temporary_output, output)
    return visited, imported


def main() -> None:
    arguments = parse_args()
    with tempfile.TemporaryDirectory(prefix="bokheim-lcc-") as temporary:
        archive = obtain_archive(arguments.archive, Path(temporary))
        visited, imported = build_database(archive, arguments.output)
    print(
        f"Created {arguments.output} from {imported:,} LCC records ({visited:,} MARC records read)",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
