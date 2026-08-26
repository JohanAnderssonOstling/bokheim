#!/usr/bin/env python3
"""Generate the curated QC Electricity and Magnetism expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path


PARENT_ID = "unified:ba57aa89-3b49-51fb-a0d4-4c0e31062732"
QUANTUM_THEORY_ID = "unified:3e955b95-69e0-5f70-81bf-0f3e66d532f3"
PARTICLE_PHYSICS_ID = "unified:2c271492-4980-5c66-a01c-db63d9e0bcf5"
PARTICLE_CAPTION = "Physics of electrons, protons, and other particles"

GROUPS = {
    "Electricity": [
        ("reference-history-study", "Reference, History & Study", {
            "Periodicals, societies, congresses, serial publications", "Collected works (nonserial)",
            "Dictionaries and encyclopedias", "Philosophy. Theory", "Nomenclature, terminology, notation, abbreviations",
            "History", "Biography", "Early works", "General works, treatises, and advanced textbooks",
            "Elementary textbooks", "Popular works", "Juvenile works", "Addresses, essays, lectures",
            "Special aspects of the subject as a whole", "Handbooks, tables, formulas, etc.",
            "Study and teaching. Research", "Miscellany and curiosa",
        }),
        ("measurement-instruments", "Measurement, Instruments & Laboratories", {
            "Laboratories", "Electric measurements", "Instruments and apparatus (General)",
        }),
        ("electrostatics-current", "Electrostatics & Electric Current", {
            "Electrostatics. Frictional electricity", "Electric current (General)",
        }),
        ("electrodynamics-quantum", "Electrodynamics, Waves & Quantum Effects", {
            "Electrodynamics", "Electric oscillations. Electric waves", "Quantum electrodynamics", "Quantum electronics",
        }),
        ("discharge-plasma-particles", "Discharge, Plasma & Particle Physics", {
            "Field emission", "Electric discharge", "Ion flow dynamics", "Plasma physics. Ionized gases", PARTICLE_CAPTION,
        }),
    ],
    "Magnetism": [
        ("reference-history-study", "Reference, History & Study", {
            "Periodicals, societies, congresses, serial publications", "Collected works (nonserial)", "Philosophy. Theory",
            "History", "Early works through 1800", "General works, treatises, and textbooks", "Popular works",
            "Juvenile works", "Recreations", "Addresses, essays, lectures", "Special aspects of the subject as a whole",
            "Study and teaching. Research",
        }),
        ("measurement-instruments", "Measurement & Instruments", {
            "Magnetic measurements", "Instruments and apparatus",
        }),
        ("magnetic-phenomena", "Magnetic Phenomena", {
            "Magnets", "Electromagnetism", "Ferrimagnetism", "Ferromagnetism. Antiferromagnetism", "Thermomagnetism",
            "Nuclear magnetism", "Paramagnetism", "Diamagnetism", "Magnetic materials",
        }),
    ],
}

LABEL_OVERRIDES = {
    "Addresses, essays, lectures": "Essays, Lectures & Addresses",
    "Collected works (nonserial)": "Collected Works",
    "Dictionaries and encyclopedias": "Dictionaries & Encyclopedias",
    "Early works through 1800": "Early Works Through 1800",
    "Electric current (General)": "Electric Current",
    "Electric oscillations. Electric waves": "Electric Oscillations & Waves",
    "Electrostatics. Frictional electricity": "Electrostatics & Frictional Electricity",
    "Ferromagnetism. Antiferromagnetism": "Ferromagnetism & Antiferromagnetism",
    "General works, treatises, and advanced textbooks": "General Works & Advanced Textbooks",
    "General works, treatises, and textbooks": "General Works & Textbooks",
    "Handbooks, tables, formulas, etc.": "Handbooks, Tables & Formulas",
    "Instruments and apparatus (General)": "Instruments & Apparatus",
    "Instruments and apparatus": "Instruments & Apparatus",
    "Nomenclature, terminology, notation, abbreviations": "Terminology, Notation & Abbreviations",
    "Periodicals, societies, congresses, serial publications": "Periodicals, Societies & Congresses",
    "Philosophy. Theory": "Philosophy & Theory",
    "Physics of electrons, protons, and other particles": "Nuclear & Particle Physics",
    "Plasma physics. Ionized gases": "Plasma Physics & Ionized Gases",
    "Special aspects of the subject as a whole": "General Special Aspects",
    "Study and teaching. Research": "Study, Teaching & Research",
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
    for word in ("And", "As", "In", "Of", "Or", "The", "Through", "To"):
        value = re.sub(rf"(?<= )({word})(?= )", word.lower(), value)
    return value


def load_records(snapshot: Path) -> dict[str, list[tuple[str, str, str]]]:
    query = """
        SELECT h.label, cr.caption, span.start_number, coalesce(span.end_number, span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h ON h.record_id = cr.record_id
        LEFT JOIN caption_hierarchy next_h ON next_h.record_id = h.record_id AND next_h.ordinal = h.ordinal + 1
        JOIN classification_span span ON span.record_id = cr.record_id AND span.ordinal = 0
        WHERE h.label IN ('Electricity', 'Magnetism') AND next_h.record_id IS NULL
          AND cr.caption <> h.label AND span.start_number GLOB 'QC*'
          AND cr.caption <> 'Special topics, A-Z'
    """
    result = {"Electricity": [], "Magnetism": []}
    with sqlite3.connect(snapshot) as connection:
        for parent, caption, start, end in connection.execute(query):
            result[parent].append((caption, start, end))
    return result


def render(records: dict[str, list[tuple[str, str, str]]]) -> str:
    expected = {"Electricity": 31, "Magnetism": 23}
    if {parent: len(rows) for parent, rows in records.items()} != expected:
        raise SystemExit(f"unexpected immediate-child counts: { {parent: len(rows) for parent, rows in records.items()} }")

    group_by_caption = {}
    for parent, groups in GROUPS.items():
        for group_slug, _, captions in groups:
            for caption in captions:
                key = (parent, caption)
                if key in group_by_caption:
                    raise SystemExit(f"duplicate group assignment for {key}")
                group_by_caption[key] = group_slug
    actual = {(parent, caption) for parent, rows in records.items() for caption, _, _ in rows}
    if actual != set(group_by_caption):
        raise SystemExit(f"group assignments differ: missing={actual - set(group_by_caption)!r}, stale={set(group_by_caption) - actual!r}")

    root_id = {parent: f"bokheim:physics:{parent.lower()}" for parent in records}
    group_id = lambda parent, group_slug: f"bokheim:physics:{parent.lower()}:{group_slug}"
    topic_id = lambda parent, caption: PARTICLE_PHYSICS_ID if caption == PARTICLE_CAPTION else f"bokheim:physics:{parent.lower()}:{slug(caption)}"

    lines = [
        "-- Expand the physical-science QC structure beneath Electricity & Electromagnetism.",
        "-- Two alphabetic Special topics, A-Z filing guides are intentionally excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-electricity-magnetism-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts = [(root_id[parent], parent) for parent in ("Electricity", "Magnetism")]
    concepts += [(group_id(parent, gid), glabel) for parent in ("Electricity", "Magnetism") for gid, glabel, _ in GROUPS[parent]]
    concepts += [(topic_id(parent, caption), display_label(caption)) for parent in ("Electricity", "Magnetism") for caption, _, _ in records[parent] if caption != PARTICLE_CAPTION]
    lines += [f"    ({sql(cid)}, {sql(label)}){',' if i + 1 < len(concepts) else ';'}" for i, (cid, label) in enumerate(concepts)]

    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES"]
    edges = [(root_id["Electricity"], PARENT_ID, 0, 10), (root_id["Magnetism"], PARENT_ID, 0, 20)]
    for parent in ("Electricity", "Magnetism"):
        rows_by_caption = {caption: (start, end) for caption, start, end in records[parent]}
        for gi, (gid, _, captions) in enumerate(GROUPS[parent], 1):
            gid_full = group_id(parent, gid)
            edges.append((gid_full, root_id[parent], 0, gi * 10))
            ordered = sorted(captions, key=lambda caption: rows_by_caption[caption][0])
            for ti, caption in enumerate(ordered, 1):
                edges.append((topic_id(parent, caption), gid_full, 1 if caption == PARTICLE_CAPTION else 0, ti * 10))
    qed_id = topic_id("Electricity", "Quantum electrodynamics")
    edges.append((qed_id, QUANTUM_THEORY_ID, 1, 10))
    lines += [f"    ({sql(cid)}, {sql(pid)}, {ordinal}, {order}){',' if i + 1 < len(edges) else ';'}" for i, (cid, pid, ordinal, order) in enumerate(edges)]

    lines += ["", "-- Priority 5 supersedes the existing outline-level priority-4 QC ranges.",
              "INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES"]
    selectors = [(root_id["Electricity"], "QC501..QC721@4", 0), (root_id["Magnetism"], "QC750..QC766@4", 0)]
    for parent in ("Electricity", "Magnetism"):
        for caption, start, end in records[parent]:
            ordinal = 3 if caption == PARTICLE_CAPTION else 0
            selectors.append((topic_id(parent, caption), f"{start}..{end}@5", ordinal))
    lines += [f"    ({sql(cid)}, 'lcc', {sql(selector)}, {ordinal}){',' if i + 1 < len(selectors) else ';'}" for i, (cid, selector, ordinal) in enumerate(selectors)]

    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES"]
    labels = [(root_id["Electricity"], "Electricity", 0), (root_id["Magnetism"], "Magnetism", 0)]
    for parent in ("Electricity", "Magnetism"):
        for caption, _, _ in records[parent]:
            ordinal = 2 if caption == PARTICLE_CAPTION else 0
            labels.append((topic_id(parent, caption), caption, ordinal))
    lines += [f"    ({sql(cid)}, {sql(label)}, {ordinal}){',' if i + 1 < len(labels) else ';'}" for i, (cid, label, ordinal) in enumerate(labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_electricity_magnetism_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")


if __name__ == "__main__":
    main()
