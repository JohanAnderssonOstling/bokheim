"""Build the development LCC reference from complete MARC and PDF source sets.

The MARC baseline preserves unprinted structure. Every PDF page and printed
entry is retained with provenance; template instructions are not promoted to
ordinary classification codes. This module never opens a curated taxonomy.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import unicodedata
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "shared/subject-projection/data/lcc-2025-sources.json"
DEFAULT_OUTPUT = ROOT / "shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"
HEADER = re.compile(r"^([A-Z][A-Za-z0-9.\-]*)\s{2,}(.+?)\s{2,}\1$")
ROW = re.compile(r"^(\S.*?)\s{2,}(\S.*)$")
NOTE = re.compile(r"(?:For |Cf\. |Including |Class |Prefer |Subarrange|Under |Arrange |See |see |e\.g\.|e\. g\.|At |Except |Use |Do not |Table |When |If |Also |Not |Here |Works |Apply |By |The |Formerly |Prior to |Offshore |Special aspects |Spurious)")


def normalize(text: str, visual: bool = False) -> str:
    if visual:
        # Poppler encloses visual-order RTL runs in RLE/PDF controls.
        text = re.sub(r"\u202b([^\u202c]*)\u202c", lambda m: m[1][::-1], text)
    text = re.sub(r"[\u202a-\u202e\u200e\u200f]", "", text)
    return unicodedata.normalize("NFC", " ".join(text.split()))


def caption(text: str, visual: bool = False) -> str:
    text = normalize(text, visual)
    return re.sub(r"\s*(?:\(Table [^)]*\)|TABLE [A-Za-z0-9.-]+)", "", text).strip()


def expand_notation(raw: str, scope: str) -> tuple[str | None, str | None, str | None, str]:
    status, value = "active", normalize(raw)
    if value.startswith("(") and value.endswith(")"):
        value, status = value[1:-1], "obsolete"
    elif value.startswith("<") and value.endswith(">"):
        value, status = value[1:-1], "optional"
    elif value.startswith("[") and value.endswith("]"):
        value, status = value[1:-1], "reference"
    table = scope if any(c.isdigit() for c in scope) or "-" in scope else None
    if re.search(r"\bdate\b", value, re.I) or re.search(r"(^|\.)x|=", value) or not re.match(r"\d|\.[A-Z]", value):
        return None, None, table, "pattern"
    if " " in value and re.fullmatch(r"[\d.A-Za-z]+ (?:\d{4}|no\. ?\d+)", value):
        return (value if table else scope + value), None, table, status
    if not re.fullmatch(r"[\d.A-Za-z:-]+", value) or value.count("-") > 1:
        return None, None, table, "pattern"
    parts = value.split("-")
    start, end = parts[0], None
    if len(parts) == 2:
        end = parts[1]
        if not end:
            return None, None, table, "pattern"
        if re.match(r"^[A-Z]", end):
            end = start.rsplit(".", 1)[0] + "." + end if "." in start else end
        elif end.startswith("."):
            if re.match(r"^\.[A-Z]", end):
                end = re.match(r"^(\d+(?:\.\d+)?)?", start)[0] + end
            else:
                number = re.match(r"^\d+", start)
                if not number:
                    return None, None, table, "pattern"
                end = number[0] + end
    if not table:
        start, end = scope + start, scope + end if end else None
    return start, end, table, status


def pdf_pages(text: str):
    chunks = text.split("\f")
    if not chunks[-1].strip():
        chunks.pop()
    for page_number, text_page in enumerate(chunks, 1):
        lines = text_page.splitlines()
        nonblank = [l.strip() for l in lines if l.strip()]
        if not nonblank:
            yield page_number, text_page, "blank", None, []
            continue
        header = next((m for l in nonblank[:5] if (m := HEADER.match(l))), None)
        if header is None:
            kind = "index" if "INDEX" in nonblank[0] else "cross_reference"
            numbered = [l for l in lines if l and not l[0].isspace() and re.match(r"[\d.(<\[]", l) and ROW.match(l)]
            if kind != "index" and numbered:
                raise ValueError(f"Unrecognized schedule header on page {page_number}: {nonblank[0]}")
            yield page_number, text_page, kind, None, []
            continue
        scope, title = header.groups()
        entries, stack, skip = [], [], set()
        for i, line in enumerate(lines):
            if not line.strip() or i in skip or HEADER.match(line.strip()):
                continue
            row = ROW.match(line) if not line[0].isspace() else None
            if row and re.match(r"[\d.(<\[]", row[1]):
                raw, first = row.groups()
                indent = row.start(2)
                full = first
                for j in range(i + 1, len(lines)):
                    following = lines[j]
                    if not following.strip() or not following[0].isspace():
                        break
                    depth = len(following) - len(following.lstrip())
                    if not 2 <= depth - indent <= 10 or len(line) < 65 or NOTE.match(following.strip()) or re.search(r"\bsee(?:\s|$)", following.strip()):
                        break
                    full += " " + following.strip()
                    skip.add(j)
                start, end, table, status = expand_notation(raw, scope)
                while stack and stack[-1][0] >= indent:
                    stack.pop()
                hierarchy = [label for _, label in stack]
                entries.append(dict(line=i + 1, notation=raw, start=start, end=end, table=table,
                                    status=status, caption=caption(full, True), first=caption(first, True),
                                    hierarchy=hierarchy, notes=[], raw_caption=normalize(full, True)))
                stack.append((indent, caption(full, True)))
            elif line and not line[0].isspace() and re.match(r"[\d.(<\[]", line):
                # Bare notations in example/template blocks have no independent caption.
                entries.append(dict(line=i + 1, notation=line.strip(), start=None, end=None,
                                    table=scope if any(c.isdigit() for c in scope) else None,
                                    status="pattern", caption="", first="", hierarchy=[], notes=[], raw_caption=""))
            elif line[0].isspace() and not line.strip().isdigit():
                label = normalize(line.strip(), True)
                if NOTE.match(label) or re.search(r"\bsee(?:\s|$)", label):
                    if entries:
                        entries[-1]["notes"].append(label)
                    continue
                indent = len(line) - len(line.lstrip())
                label = re.sub(r"\s*-- Continued$", "", label)
                while stack and stack[-1][0] >= indent:
                    stack.pop()
                stack.append((indent, label))
        yield page_number, text_page, "schedule", scope, entries


def approved_entries(html: str, tables: set[str]):
    from bs4 import BeautifulSoup
    soup = BeautifulSoup(html, "html.parser")
    hierarchy = {}
    for line, tr in enumerate(soup.select("tr"), 1):
        cells = tr.find_all("td", recursive=False)
        if len(cells) != 2:
            continue
        raw = normalize(cells[0].get_text(" ", strip=True))
        divs = cells[1].find_all("div", recursive=False)
        if not raw:
            for div in divs:
                level = next((int(c[2:]) for c in div.get("class", []) if re.fullmatch(r"hi\d+", c)), None)
                if level is not None:
                    hierarchy = {k: v for k, v in hierarchy.items() if k < level}
                    hierarchy[level] = normalize(div.get_text(" ", strip=True))
            continue
        text = normalize(cells[1].get_text(" ", strip=True))
        if not text:
            continue
        wrapper = raw[0] if raw.startswith(("(", "[", "<")) else ""
        body = raw[1:-1] if wrapper else raw
        first, _, rest = body.partition(" ")
        if rest and (first in tables or "-" in first):
            scope, relative = first, rest
        else:
            m = re.match(r"^([A-Z]+)(.*)$", body)
            if not m:
                raise ValueError(f"Unrecognized approved-list notation: {raw}")
            scope, relative = m.groups()
            relative = re.sub(r"\s+\.", ".", relative.strip())
        if wrapper:
            relative = wrapper + relative + {"(": ")", "[": "]", "<": ">"}[wrapper]
        start, end, table, status = expand_notation(relative, scope)
        if "CANCEL" in text:
            status = "obsolete"
        level = next((int(c[2:]) for d in divs[:1] for c in d.get("class", []) if re.fullmatch(r"hi\d+", c)), 999)
        cap = divs[0].get_text(" ", strip=True) if divs else text
        cap = re.sub(r"\s+CANCEL(?: LINE)?", "", cap)
        notes = [normalize(d.get_text(" ", strip=True)) for d in divs[1:]]
        yield dict(line=line, notation=raw, start=start, end=end, table=table, status=status,
                   caption=caption(cap), first=caption(cap), hierarchy=[v for k, v in sorted(hierarchy.items()) if k < level],
                   notes=notes, raw_caption=text)


SOURCE_SCHEMA = """
CREATE TABLE reference_document(document_id INTEGER PRIMARY KEY, filename TEXT UNIQUE NOT NULL, url TEXT NOT NULL, sha256 TEXT NOT NULL, kind TEXT NOT NULL);
CREATE TABLE reference_page(document_id INTEGER NOT NULL REFERENCES reference_document, page INTEGER NOT NULL, kind TEXT NOT NULL, scope TEXT, text TEXT NOT NULL, PRIMARY KEY(document_id,page));
CREATE TABLE reference_entry(entry_id INTEGER PRIMARY KEY, document_id INTEGER NOT NULL REFERENCES reference_document, page INTEGER NOT NULL, line INTEGER NOT NULL, notation TEXT NOT NULL, table_id TEXT, start_number TEXT, end_number TEXT, caption TEXT NOT NULL, status TEXT NOT NULL, hierarchy_json TEXT NOT NULL, notes_json TEXT NOT NULL, record_id INTEGER REFERENCES classification_record, disposition TEXT NOT NULL);
CREATE TABLE reference_record_state(record_id INTEGER PRIMARY KEY REFERENCES classification_record, status TEXT NOT NULL, entry_id INTEGER REFERENCES reference_entry);
CREATE TABLE reference_caption_history(record_id INTEGER NOT NULL REFERENCES classification_record, entry_id INTEGER NOT NULL REFERENCES reference_entry, old_caption TEXT, new_caption TEXT NOT NULL);
CREATE TABLE reference_redirect(table_id TEXT NOT NULL, code TEXT NOT NULL, target_code TEXT NOT NULL, source_url TEXT NOT NULL, PRIMARY KEY(table_id,code,target_code));
CREATE TABLE reference_note(code TEXT NOT NULL, note TEXT NOT NULL, source_url TEXT NOT NULL);
CREATE VIRTUAL TABLE reference_page_fts USING fts5(text, document_id UNINDEXED, page UNINDEXED);
"""


class Overlay:
    def __init__(self, connection: sqlite3.Connection):
        self.c = connection
        self.records = defaultdict(list)
        self.captions = {}
        self.counts = Counter()
        for rid, table, start, end, cap in connection.execute("SELECT record_id,table_id,start_number,end_number,caption FROM classification_span JOIN classification_record USING(record_id)"):
            self.records[table, start, end].append(rid)
            self.captions[rid] = normalize(cap or "")

    def entry(self, document: int, page: int, entry: dict, url: str, approved: bool = False):
        e = entry
        rid, disposition = None, "template"
        cap = e["caption"]
        key = e["table"], e["start"], e["end"]
        matches = self.records.get(key, [])
        if not approved and len(matches) == 1 and e["notes"]:
            # A wrapped "By ..." clause may also be formatted like a note.
            # Retain it when the source wording exactly reconstructs the MARC caption.
            if normalize(cap + " " + e["notes"][0]) == self.captions[matches[0]]:
                cap = self.captions[matches[0]]
        if e["start"] and e["status"] != "pattern":
            if e["status"] == "reference":
                disposition = "cross_reference"
            else:
                rid = next((r for r in matches if self.captions[r] == cap), None)
                if rid is None and not approved:
                    # A following indented annotation must not extend a complete known caption.
                    rid = next((r for r in matches if self.captions[r] == e["first"]), None)
                    if rid is not None:
                        cap = e["first"]
                if rid is None and len(matches) == 1:
                    rid = matches[0]
                if rid is None:
                    rid = self.c.execute("INSERT INTO classification_record(control_number,record_status,scheme,edition,caption) VALUES(?,'n','lcc',NULL,?)", (f"local:2025:{document}:{page}:{e['line']}", cap)).lastrowid
                    self.c.execute("INSERT INTO classification_span VALUES(?,0,?,?,?)", (rid, e["table"], e["start"], e["end"]))
                    self.c.executemany("INSERT INTO caption_hierarchy VALUES(?,?,'caption',?)", [(rid, i, value) for i, value in enumerate(e["hierarchy"])])
                    self.records[key].append(rid)
                    self.captions[rid] = cap
                    disposition = "added" if e["status"] != "obsolete" else "historical_added"
                elif self.captions[rid] != cap and e["status"] != "obsolete":
                    disposition = "caption_updated"
                else:
                    disposition = "matched"
        entry_id = self.c.execute("INSERT INTO reference_entry(document_id,page,line,notation,table_id,start_number,end_number,caption,status,hierarchy_json,notes_json,record_id,disposition) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
                                  (document, page, e["line"], e["notation"], e["table"], e["start"], e["end"], cap, e["status"], json.dumps(e["hierarchy"], ensure_ascii=False), json.dumps(e["notes"], ensure_ascii=False), rid, disposition)).lastrowid
        if rid:
            if disposition == "caption_updated":
                self.c.execute("INSERT INTO reference_caption_history VALUES(?,?,?,?)", (rid, entry_id, self.captions[rid], cap))
                self.c.execute("UPDATE classification_record SET caption=? WHERE record_id=?", (cap, rid))
                self.captions[rid] = cap
            self.c.execute("INSERT INTO reference_record_state VALUES(?,?,?) ON CONFLICT(record_id) DO UPDATE SET status=excluded.status, entry_id=excluded.entry_id", (rid, e["status"], entry_id))
        if e["status"] in ("obsolete", "reference") and e["start"]:
            for target in re.findall(r"\bsee ([A-Z]{1,3}\d+(?:\.[A-Za-z0-9]+)*)", " ".join([e["raw_caption"], *e["notes"]]), re.I):
                self.c.execute("INSERT OR REPLACE INTO reference_redirect VALUES(?,?,?,?)", (e["table"] or "", e["start"], target, url))
        for note in e["notes"] if approved else []:
            if e["start"]:
                self.c.execute("INSERT INTO reference_note VALUES(?,?,?)", (e["start"], note, url))
        self.counts[disposition] += 1


def verify_sources(sources: Path, manifest: dict):
    for item in [manifest["base_archive"], *manifest["schedules"], *manifest["approved_lists"]]:
        path = sources / item["filename"]
        if hashlib.sha256(path.read_bytes()).hexdigest() != item["sha256"]:
            raise ValueError(f"Source checksum mismatch: {path}")


def validate_database(connection: sqlite3.Connection, manifest: dict):
    assert connection.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    assert not connection.execute("PRAGMA foreign_key_check").fetchall()
    expected = {item["filename"]: (item["url"], item["sha256"]) for item in [*manifest["schedules"], *manifest["approved_lists"]]}
    actual = {name: (url, digest) for name, url, digest in connection.execute("SELECT filename,url,sha256 FROM reference_document")}
    assert actual == expected, "Reference document coverage differs from the pinned source manifest"
    for item in manifest["schedules"]:
        count = connection.execute("SELECT count(*) FROM reference_page p JOIN reference_document d USING(document_id) WHERE d.filename=?", (item["filename"],)).fetchone()[0]
        assert count == item["pages"], item["filename"]
    assert connection.execute("SELECT count(*) FROM reference_page_fts").fetchone()[0] == sum(item["pages"] for item in manifest["schedules"])
    assert connection.execute("SELECT count(*) FROM reference_entry WHERE status='pattern' AND record_id IS NOT NULL").fetchone()[0] == 0
    assert connection.execute("SELECT value FROM metadata WHERE key='reference_source_manifest_sha256'").fetchone() == (hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),)


def publish(staging: Path, output: Path):
    """Publish a fully validated build, retaining the previous reference as backup."""
    with sqlite3.connect(staging.as_uri() + "?mode=ro", uri=True) as ready:
        validate_database(ready, json.loads(MANIFEST.read_text()))
    ready.close()
    if output.exists():
        with sqlite3.connect(output) as current:
            if current.execute("SELECT 1 FROM sqlite_master WHERE name='concept'").fetchone() or not current.execute("SELECT 1 FROM sqlite_master WHERE name='classification_record'").fetchone():
                raise ValueError("Refusing to replace a non-reference database")
            backup = output.with_suffix(".before-complete-2025.sqlite3")
            if not backup.exists():
                with sqlite3.connect(backup) as saved:
                    current.backup(saved)
                saved.close()
            if current.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()[0] != 0:
                raise ValueError("Existing reference is busy; the validated build has not been published")
        current.close()
    os.replace(staging, output)
    print(f"Published reference: {output}", flush=True)


def base_database(sources: Path, manifest: dict, output: Path, cache: Path | None):
    if cache:
        with sqlite3.connect(cache.as_uri() + "?mode=ro", uri=True) as source, sqlite3.connect(output) as target:
            assert source.execute("SELECT value FROM metadata WHERE key='source_sha256'").fetchone() == (manifest["base_archive"]["sha256"],)
            assert source.execute("SELECT count(*) FROM classification_record").fetchone()[0] == 652241
            assert not source.execute("SELECT 1 FROM sqlite_master WHERE name='reference_document'").fetchone()
            source.backup(target)
        source.close()
        target.close()
        return
    spec = importlib.util.spec_from_file_location("lcc_marc_import", ROOT / "scripts/import-lcc-mdsconnect.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with sqlite3.connect(output) as connection:
        connection.executescript(module.SCHEMA)
        connection.executemany("INSERT INTO metadata VALUES(?,?)", [("source_release", "MDSConnect Classification 2019 retrospective"), ("source_url", manifest["base_archive"]["url"]), ("source_sha256", manifest["base_archive"]["sha256"]), ("format_version", "1")])
        count = 0
        with gzip.open(sources / manifest["base_archive"]["filename"], "rb") as stream:
            for _, element in ET.iterparse(stream, events=("end",)):
                if element.tag == module.MARC + "record":
                    count += module.import_record(connection, element)
                    element.clear()
                    if count % 100000 == 0:
                        connection.commit()
                        print(f"MARC records: {count}", flush=True)
        assert count == 652241, count
        connection.commit()
        connection.executescript("CREATE INDEX classification_span_start_idx ON classification_span(start_number,end_number); CREATE INDEX classification_span_table_idx ON classification_span(table_id,start_number); CREATE INDEX caption_hierarchy_label_idx ON caption_hierarchy(label,record_id);")


def build(sources: Path, output: Path, cache: Path | None = None):
    manifest = json.loads(MANIFEST.read_text())
    verify_sources(sources, manifest)
    output.parent.mkdir(parents=True, exist_ok=True)
    if cache and output == cache:
        raise ValueError("The output cannot replace the baseline cache")
    if output.exists():
        with sqlite3.connect(output.as_uri() + "?mode=ro", uri=True) as current:
            if current.execute("SELECT 1 FROM sqlite_master WHERE name='concept'").fetchone():
                raise ValueError("Refusing to replace a curated taxonomy")
            if not current.execute("SELECT 1 FROM sqlite_master WHERE name='classification_record'").fetchone():
                raise ValueError("Refusing to replace a database that is not an LCC reference")
        current.close()
    staging = output.with_name(f".{output.name}.{os.getpid()}.tmp")
    if staging.exists():
        raise ValueError(f"Staging file exists: {staging}")
    base_database(sources, manifest, staging, cache)
    connection = sqlite3.connect(staging)
    try:
        connection.execute("PRAGMA foreign_keys=ON")
        connection.executescript(SOURCE_SCHEMA)
        overlay = Overlay(connection)
        tables = {r[0] for r in connection.execute("SELECT DISTINCT table_id FROM classification_span WHERE table_id IS NOT NULL")}
        pages_total = 0
        for item in manifest["schedules"]:
            doc = connection.execute("INSERT INTO reference_document(filename,url,sha256,kind) VALUES(?,?,?,'schedule')", (item["filename"], item["url"], item["sha256"])).lastrowid
            result = subprocess.run(["pdftotext", "-layout", str(sources / item["filename"]), "-"], capture_output=True, check=True)
            pages = list(pdf_pages(result.stdout.decode("utf-8")))
            assert len(pages) == item["pages"], (item["filename"], len(pages), item["pages"])
            for number, text, kind, scope, entries in pages:
                connection.execute("INSERT INTO reference_page VALUES(?,?,?,?,?)", (doc, number, kind, scope, text))
                connection.execute("INSERT INTO reference_page_fts VALUES(?,?,?)", (text, doc, number))
                for entry in entries:
                    overlay.entry(doc, number, entry, item["url"] + f"#page={number}")
            connection.commit()
            pages_total += len(pages)
            print(f"Imported {item['filename']}: {len(pages)} pages", flush=True)
        for item in manifest["approved_lists"]:
            doc = connection.execute("INSERT INTO reference_document(filename,url,sha256,kind) VALUES(?,?,?,'approved_list')", (item["filename"], item["url"], item["sha256"])).lastrowid
            html = (sources / item["filename"]).read_text()
            entries = list(approved_entries(html, tables))
            connection.execute("INSERT INTO reference_page VALUES(?,1,'approved_list',NULL,?)", (doc, html))
            for entry in entries:
                overlay.entry(doc, 1, entry, item["url"], approved=True)
            connection.commit()
            print(f"Imported {item['filename']}: {len(entries)} entries", flush=True)
        assert pages_total == sum(item["pages"] for item in manifest["schedules"])
        connection.executemany("INSERT OR REPLACE INTO metadata VALUES(?,?)", [
            ("reference_edition", "2025 schedules with all 20 published 2025 approved lists"),
            ("reference_source_manifest_sha256", hashlib.sha256(MANIFEST.read_bytes()).hexdigest()),
            ("reference_coverage", "All 45 text schedules and supplementary tables; all 20953 pages preserved. MARC 2019 structure retained for entries not printed in the schedules. Relative templates are stored in reference_entry rather than treated as ordinary codes."),
            ("reference_update_counts", json.dumps(overlay.counts, sort_keys=True)),
            ("format_version", "2"),
        ])
        connection.commit()
        connection.executescript("CREATE INDEX reference_entry_code_idx ON reference_entry(table_id,start_number,end_number); CREATE INDEX reference_entry_record_idx ON reference_entry(record_id); CREATE VIEW reference_current_record AS SELECT r.* FROM classification_record r LEFT JOIN reference_record_state s USING(record_id) WHERE coalesce(s.status,'active') != 'obsolete';")
        assert connection.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
        assert not connection.execute("PRAGMA foreign_key_check").fetchall()
        connection.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        connection.execute("PRAGMA journal_mode=DELETE")
        print(json.dumps(dict(overlay.counts), sort_keys=True), flush=True)
    except BaseException:
        connection.close()
        # Leave the isolated failed build available for diagnosis; never publish it.
        raise
    connection.close()
    publish(staging, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sources", required=True, type=Path, help="Directory containing the verified source files")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--baseline-cache", type=Path, help="Optional validated, unmodified 2019 MARC database")
    args = parser.parse_args()
    build(args.sources.resolve(), args.output.resolve(), args.baseline_cache.resolve() if args.baseline_cache else None)


if __name__ == "__main__":
    main()
