#!/usr/bin/env python3
"""Generate the reconciled Veterinary Medicine expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path


CANONICAL_ID = "unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82"
BISAC_DUPLICATE_ID = "unified:3d0a37fc-a3aa-58e5-bba9-677c9ec8f8e6"
LEGAL_ID = "unified:e3a8f859-86c0-5d2c-920c-609508aeccc8"
SURGERY_ID = "unified:a41a1d8c-7a93-57be-81e2-4541085a65bb"
SURGERY_CAPTION = "Veterinary surgery"

GROUPS = [
    ("reference-history-profession", "Reference, History & Professional Study", {
        "Exhibitions", "Periodicals. Societies. Serials", "Congresses", "Collections", "Dictionaries. Encyclopedias",
        "Nomenclature. Terminology. Abbreviations", "Communication in veterinary medicine", "Directories", "Biography",
        "History and conditions", "General works", "Addresses, essays, lectures", "Handbooks, manuals, etc.",
        "Manuals for the non-professional", "Juvenile works", "Vocational guidance", "Study and teaching. Research",
        "Miscellaneous", "Tables, examinations, questions, etc.", "Miscellaneous pamphlets",
    }),
    ("practice-ethics-public-health", "Practice, Ethics & Public Health", {
        "Veterinary hospitals", "Veterinary public health", "Alternative veterinary medicine. Holistic veterinary medicine",
        "Veterinary ethics", "Euthanasia", "Economics of veterinary practice", "Veterinary hygiene",
        "Animal health technology", "Laws. Veterinary jurisprudence",
    }),
    ("biomedical-foundations", "Genetics, Behavior & Biomedical Foundations", {
        "Veterinary genetics", "Veterinary ethology", "Veterinary psychopathology", "Veterinary immunology",
        "Veterinary cytology", "Veterinary histology", "Veterinary toxicology",
        "Veterinary radiology. Veterinary nuclear medicine. Veterinary radiography", "Veterinary anatomy",
        "Veterinary embryology", "Veterinary physiology", "Veterinary endocrinology", "Veterinary geriatrics",
        "Veterinary pathology", "Veterinary forensic medicine", "Veterinary hematology",
    }),
    ("diagnosis-emergency-disease", "Diagnosis, Emergency Care & Disease", {
        "Diagnosis", "Veterinary emergencies. Veterinary critical care. First aid for animals",
        "Veterinary epidemiology. Epizootiology", "Communicable diseases of animals", "Predatory animals and their control",
    }),
    ("microbiology-parasitology", "Microbiology & Parasitology", {
        "Veterinary microbiology", "Veterinary bacteriology", "Veterinary virology", "Veterinary protozoology",
        "Veterinary mycology", "Veterinary parasitology",
    }),
    ("organ-systems-orthopedics", "Organ Systems & Orthopedics", {
        "Veterinary medicine of special organs, regions, and systems", "Veterinary orthopedics",
    }),
    ("pharmacology-therapies", "Pharmacology & Therapies", {
        "Veterinary acupuncture", "Veterinary pharmacology", "Veterinary physical medicine",
    }),
    ("animal-specific-medicine", "Animal-Specific Medicine", {"Diseases of special classes of animals"}),
]

LABEL_OVERRIDES = {
    "Addresses, essays, lectures": "Essays, Lectures & Addresses",
    "Alternative veterinary medicine. Holistic veterinary medicine": "Alternative & Holistic Veterinary Medicine",
    "Dictionaries. Encyclopedias": "Dictionaries & Encyclopedias",
    "Handbooks, manuals, etc.": "Handbooks & Manuals",
    "History and conditions": "History & Conditions",
    "Laws. Veterinary jurisprudence": "Veterinary Law & Jurisprudence",
    "Nomenclature. Terminology. Abbreviations": "Terminology & Abbreviations",
    "Periodicals. Societies. Serials": "Periodicals, Societies & Serials",
    "Study and teaching. Research": "Study, Teaching & Research",
    "Tables, examinations, questions, etc.": "Tables, Examinations & Questions",
    "Veterinary emergencies. Veterinary critical care. First aid for animals": "Veterinary Emergency & Critical Care",
    "Veterinary epidemiology. Epizootiology": "Veterinary Epidemiology & Epizootiology",
    "Veterinary medicine of special organs, regions, and systems": "Veterinary Organ-System Medicine",
    "Veterinary radiology. Veterinary nuclear medicine. Veterinary radiography": "Veterinary Radiology & Nuclear Medicine",
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
    for word in ("And", "As", "For", "In", "Of", "Or", "The", "To"):
        value = re.sub(rf"(?<= )({word})(?= )", word.lower(), value)
    return value


def load_records(snapshot: Path) -> list[tuple[str, str, str]]:
    query = """
        SELECT cr.caption, span.start_number, coalesce(span.end_number, span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h ON h.record_id = cr.record_id
        LEFT JOIN caption_hierarchy next_h ON next_h.record_id = h.record_id AND next_h.ordinal = h.ordinal + 1
        JOIN classification_span span ON span.record_id = cr.record_id AND span.ordinal = 0
        WHERE h.label = 'Veterinary medicine' AND next_h.record_id IS NULL
          AND cr.caption <> h.label AND span.start_number GLOB 'SF*' AND cr.caption NOT LIKE '%A-Z%'
    """
    with sqlite3.connect(snapshot) as connection:
        return connection.execute(query).fetchall()


def render(records: list[tuple[str, str, str]]) -> str:
    if len(records) != 63:
        raise SystemExit(f"expected 63 semantic Veterinary medicine records, found {len(records)}")
    assigned = {caption: gid for gid, _, captions in GROUPS for caption in captions}
    expected = {caption for caption, _, _ in records if caption != SURGERY_CAPTION}
    if set(assigned) != expected:
        raise SystemExit(f"group assignments differ: missing={expected - set(assigned)!r}, stale={set(assigned) - expected!r}")

    group_id = lambda gid: f"bokheim:veterinary:{gid}"
    topic_id = lambda caption: SURGERY_ID if caption == SURGERY_CAPTION else f"bokheim:veterinary:{slug(caption)}"
    rows = {caption: (start, end) for caption, start, end in records}

    lines = [
        "-- Reconcile the BISAC and LCC Veterinary Medicine concepts, distinguish",
        "-- veterinary law, and add the semantic immediate SF subdivisions.",
        "-- Two alphabetic A-Z filing guides are intentionally excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-veterinary-medicine-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        "-- Move the BISAC route, selector, and existing children to the LCC-backed canonical concept.",
        f"INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) SELECT {sql(CANONICAL_ID)}, parent_concept_id, 1, display_order FROM concept_parent WHERE concept_id={sql(BISAC_DUPLICATE_ID)};",
        f"UPDATE OR IGNORE concept_parent SET parent_concept_id={sql(CANONICAL_ID)} WHERE parent_concept_id={sql(BISAC_DUPLICATE_ID)};",
        f"INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES ({sql(CANONICAL_ID)}, 'bisac', 'MED089000', 0);",
        f"DELETE FROM concept_parent WHERE concept_id={sql(BISAC_DUPLICATE_ID)} OR parent_concept_id={sql(BISAC_DUPLICATE_ID)};",
        f"DELETE FROM concept WHERE concept_id={sql(BISAC_DUPLICATE_ID)};",
        f"UPDATE concept SET preferred_label='Veterinary Medicine' WHERE concept_id={sql(CANONICAL_ID)};",
        f"UPDATE concept SET preferred_label='Veterinary Law & Regulation' WHERE concept_id={sql(LEGAL_ID)};",
        f"INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES ({sql(LEGAL_ID)}, 'Veterinary medicine', 0);", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts = [(group_id(gid), glabel) for gid, glabel, _ in GROUPS]
    concepts += [(topic_id(caption), display_label(caption)) for caption, _, _ in records if caption != SURGERY_CAPTION]
    lines += [f"    ({sql(cid)}, {sql(label)}){',' if i + 1 < len(concepts) else ';'}" for i, (cid, label) in enumerate(concepts)]

    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES"]
    edges = []
    for gi, (gid, _, captions) in enumerate(GROUPS, 1):
        gid_full = group_id(gid)
        edges.append((gid_full, CANONICAL_ID, 0, 100 + gi * 10))
        for ti, caption in enumerate(sorted(captions, key=lambda value: rows[value][0]), 1):
            edges.append((topic_id(caption), gid_full, 0, ti * 10))
    lines += [f"    ({sql(cid)}, {sql(pid)}, {ordinal}, {order}){',' if i + 1 < len(edges) else ';'}" for i, (cid, pid, ordinal, order) in enumerate(edges)]

    lines += ["", "-- Surgery already exists as a direct BISAC child and is reused without a duplicate route.",
              "-- Priority 4 supersedes the existing priority-3 outline ranges.",
              "INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES"]
    selectors = [(topic_id(caption), f"{start}..{end}@4", 0) for caption, start, end in records]
    lines += [f"    ({sql(cid)}, 'lcc', {sql(selector)}, {ordinal}){',' if i + 1 < len(selectors) else ';'}" for i, (cid, selector, ordinal) in enumerate(selectors)]

    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES"]
    labels = [(topic_id(caption), caption, 0) for caption, _, _ in records]
    lines += [f"    ({sql(cid)}, {sql(label)}, {ordinal}){',' if i + 1 < len(labels) else ';'}" for i, (cid, label, ordinal) in enumerate(labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("client/library-model/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("client/library-model/data/migrations/20260817_veterinary_medicine_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")


if __name__ == "__main__":
    main()
