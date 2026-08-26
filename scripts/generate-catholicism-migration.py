#!/usr/bin/env python3
"""Generate the curated Catholicism LCC expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path


PARENT_ID = "unified:9e5893fb-bd11-51e6-a9ee-6163f7b5e993"
LITURGY_ID = "unified:f5fd5565-1452-5c3b-a113-671f57f330d9"
CANON_LAW_ID = "unified:e8852e6c-2231-5163-8147-79489bb9b28f"

REUSE = {
    "Liturgy and ritual": LITURGY_ID,
    "Canon law": CANON_LAW_ID,
}
REUSE_SELECTOR_ORDINAL = {"Liturgy and ritual": None, "Canon law": 2}
REUSE_LABEL_ORDINAL = {"Liturgy and ritual": 0, "Canon law": 1}

GROUPS = [
    ("reference-history-study", "Reference, History & Study", {
        "Periodicals", "Societies. Confraternities, etc.", "Gesellenvereine", "Secular institutes", "Personal prelatures",
        "Councils", "Museums. Exhibitions", "Dictionaries. Encyclopedias", "Handbooks, manuals, etc.",
        "Computer network resources", "Pictorial works", "Directories. Yearbooks", "Atlases", "Documents",
        "General collected works", "Study and teaching", "Study and teaching of Church history", "History",
    }),
    ("doctrine-preaching-relations", "Doctrine, Preaching & Religious Relations", {
        "Sermons", "Moral theology. Casuistry. Cases of conscience, etc.", "Theology. Doctrine. Dogmatics",
        "Controversial works against the Catholic Church", "Joint debates and discussions on Catholicism",
        "Controversial works by Catholics against the Catholic Church", "Catholic Church and other churches",
        "Catholic Church and occult sciences", "Catholic Church and the state",
    }),
    ("governance-clergy-administration", "Governance, Clergy & Administration", {
        "Government and organization of the Catholic Church", "Costume. Vestments. Liturgical objects", "Tonsure. Hair. Beard",
        "Rules and ordinances for the clergy", "The cathedral", "The church. Sanctity, etc.", "Canon law",
        "Manuals and forms for secretaries, etc.", "Parish records, accounts, etc.", "Finances, subsidies, etc.", "Benefices",
    }),
    ("worship-devotion-religious-life", "Worship, Devotion & Religious Life", {
        "Creeds and catechisms", "Forms of worship. Catholic practice", "Liturgy and ritual",
        "Meditations. Devotional readings. Spiritual exercises, etc.", "Sacraments of the Catholic Church", "Sacramentals",
        "Images", "Relics. Veneration, etc.", "Shrines. Holy Places", "Pilgrimages", "Processions", "Saints. Hagiology",
        "Exorcisms", "Votive offerings", "Practical religion", "Christian life", "Religious life. Religious state",
    }),
    ("institutions-traditions-biography", "Institutions, Traditions & Biography", {
        "Monasticism. Religious orders", "Churches, cathedrals, abbeys (as parish churches), etc.",
        "Biography and portraits", "Eastern Rite Churches in communion with the Holy See of Rome",
        "Dissenting sects other than Protestant",
    }),
]

LABEL_OVERRIDES = {
    "Biography and portraits": "Biography & Portraits",
    "Catholic Church and occult sciences": "Catholic Church & Occult Sciences",
    "Catholic Church and other churches": "Catholic Church & Other Churches",
    "Catholic Church and the state": "Catholic Church & the State",
    "Churches, cathedrals, abbeys (as parish churches), etc.": "Churches, Cathedrals & Abbeys",
    "Costume. Vestments. Liturgical objects": "Clerical Costume, Vestments & Liturgical Objects",
    "Creeds and catechisms": "Creeds & Catechisms",
    "Dictionaries. Encyclopedias": "Dictionaries & Encyclopedias",
    "Directories. Yearbooks": "Directories & Yearbooks",
    "Eastern Rite Churches in communion with the Holy See of Rome": "Eastern Catholic Churches",
    "Finances, subsidies, etc.": "Finances & Subsidies",
    "Forms of worship. Catholic practice": "Catholic Worship & Practice",
    "Government and organization of the Catholic Church": "Catholic Church Governance & Organization",
    "Handbooks, manuals, etc.": "Handbooks & Manuals",
    "Joint debates and discussions on Catholicism": "Debates & Discussions on Catholicism",
    "Meditations. Devotional readings. Spiritual exercises, etc.": "Meditation, Devotional Reading & Spiritual Exercises",
    "Monasticism. Religious orders": "Catholic Monasticism & Religious Orders",
    "Moral theology. Casuistry. Cases of conscience, etc.": "Catholic Moral Theology & Casuistry",
    "Museums. Exhibitions": "Museums & Exhibitions",
    "Parish records, accounts, etc.": "Parish Records & Accounts",
    "Relics. Veneration, etc.": "Relics & Veneration",
    "Religious life. Religious state": "Catholic Religious Life",
    "Saints. Hagiology": "Catholic Saints & Hagiology",
    "Shrines. Holy Places": "Shrines & Holy Places",
    "Societies. Confraternities, etc.": "Societies & Confraternities",
    "Study and teaching of Church history": "Study & Teaching of Church History",
    "The church. Sanctity, etc.": "Sanctity of the Church",
    "Theology. Doctrine. Dogmatics": "Catholic Theology, Doctrine & Dogmatics",
    "Tonsure. Hair. Beard": "Tonsure, Hair & Beard",
}


def slug(value: str) -> str:
    value = unicodedata.normalize("NFKD", value).encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def display_label(caption: str) -> str:
    if caption in LABEL_OVERRIDES:
        return LABEL_OVERRIDES[caption]
    value = caption.replace(". ", ", ").replace(" and ", " & ").rstrip(".").title()
    for word in ("And", "As", "By", "For", "In", "Of", "Or", "The", "To"):
        value = re.sub(rf"(?<= )({word})(?= )", word.lower(), value)
    return value


def load_records(snapshot: Path) -> list[tuple[str, str, str]]:
    query = """
        SELECT cr.caption, span.start_number, coalesce(span.end_number, span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h ON h.record_id = cr.record_id
        LEFT JOIN caption_hierarchy next_h ON next_h.record_id = h.record_id AND next_h.ordinal = h.ordinal + 1
        JOIN classification_span span ON span.record_id = cr.record_id AND span.ordinal = 0
        WHERE h.label = 'Catholic Church' AND next_h.record_id IS NULL
          AND cr.caption <> h.label AND span.start_number GLOB 'BX*' AND cr.caption NOT LIKE '%A-Z%'
    """
    with sqlite3.connect(snapshot) as connection:
        records = connection.execute(query).fetchall()
    normalized = []
    for caption, start, end in records:
        if not re.match(r"^[A-Z]+", end):
            letters = re.match(r"^[A-Z]+", start)
            if not letters:
                raise SystemExit(f"cannot restore omitted LCC letters for {caption}: {start}..{end}")
            end = letters.group(0) + end
        normalized.append((caption, start, end))
    return normalized


def render(records: list[tuple[str, str, str]]) -> str:
    if len(records) != 60:
        raise SystemExit(f"expected 60 semantic Catholic Church records, found {len(records)}")
    assigned = {caption: gid for gid, _, captions in GROUPS for caption in captions}
    actual = {caption for caption, _, _ in records}
    if set(assigned) != actual:
        raise SystemExit(f"group assignments differ: missing={actual - set(assigned)!r}, stale={set(assigned) - actual!r}")

    group_id = lambda gid: f"bokheim:catholic:{gid}"
    topic_id = lambda caption: REUSE.get(caption, f"bokheim:catholic:{slug(caption)}")
    rows = {caption: (start, end) for caption, start, end in records}

    lines = [
        "-- Add the semantic immediate Catholic Church BX subdivisions.",
        "-- Two alphabetic A-Z filing guides are intentionally excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-catholicism-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        f"UPDATE concept SET preferred_label='Liturgy & Ritual' WHERE concept_id={sql(LITURGY_ID)};",
        f"UPDATE concept SET preferred_label='Catholic Canon Law' WHERE concept_id={sql(CANON_LAW_ID)};",
        f"INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES ({sql(CANON_LAW_ID)}, 'Law of the Roman Catholic Church. The Holy See', 0);", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts = [(group_id(gid), glabel) for gid, glabel, _ in GROUPS]
    concepts += [(topic_id(caption), display_label(caption)) for caption, _, _ in records if caption not in REUSE]
    lines += [f"    ({sql(cid)}, {sql(label)}){',' if i + 1 < len(concepts) else ';'}" for i, (cid, label) in enumerate(concepts)]

    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES"]
    edges = []
    for gi, (gid, _, captions) in enumerate(GROUPS, 1):
        gid_full = group_id(gid)
        edges.append((gid_full, PARENT_ID, 0, gi * 10))
        for ti, caption in enumerate(sorted(captions, key=lambda value: rows[value][0]), 1):
            edges.append((topic_id(caption), gid_full, 1 if caption in REUSE else 0, ti * 10))
    lines += [f"    ({sql(cid)}, {sql(pid)}, {ordinal}, {order}){',' if i + 1 < len(edges) else ';'}" for i, (cid, pid, ordinal, order) in enumerate(edges)]

    lines += ["", "-- Priority 4 supersedes the existing priority-3 outline ranges.",
              "INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES"]
    selectors = []
    for caption, start, end in records:
        ordinal = REUSE_SELECTOR_ORDINAL.get(caption, 0)
        if ordinal is not None:
            selectors.append((topic_id(caption), f"{start}..{end}@4", ordinal))
    lines += [f"    ({sql(cid)}, 'lcc', {sql(selector)}, {ordinal}){',' if i + 1 < len(selectors) else ';'}" for i, (cid, selector, ordinal) in enumerate(selectors)]

    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES"]
    labels = [(topic_id(caption), caption, REUSE_LABEL_ORDINAL.get(caption, 0)) for caption, _, _ in records]
    lines += [f"    ({sql(cid)}, {sql(label)}, {ordinal}){',' if i + 1 < len(labels) else ';'}" for i, (cid, label, ordinal) in enumerate(labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_catholicism_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")


if __name__ == "__main__":
    main()
