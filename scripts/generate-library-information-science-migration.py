#!/usr/bin/env python3
"""Generate the reconciled Library & Information Science expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path


DOMAIN_ID = "unified:6736dda5-b718-5b00-8204-ffea565580a1"
LIBRARIES_ID = "unified:e1781c6f-1593-5c16-a3de-0e9d3cb0d84a"
LIBRARY_SCIENCE_ID = "unified:c222fc36-cf9c-51ff-b3be-70bebf4ba99e"

RELATIONSHIPS_ID = "unified:05617c29-2b21-5f0a-aa1c-197ed438eaf4"
PHYSICAL_PLANT_ID = "unified:02eef195-5afc-55c7-9714-afc80b22533b"
PERSONNEL_ID = "unified:1f5b144f-f582-590f-98be-c63ac161e680"
OPERATIONS_ID = "unified:6de5e626-ef32-5543-a5c8-7d4b7eb00dba"
SPECIFIC_LIBRARIES_ID = "unified:d957f016-bd35-5280-a739-4a2bf4d291aa"
GENERAL_LIBRARIES_ID = "unified:eb2e398f-bf2b-5ad1-a408-4518648cbe16"
READING_MEDIA_ID = "unified:b7145630-6bf8-5412-8ca4-75906385cf04"
INFORMATION_SERVICES_ID = "unified:9370a95f-6b6c-59a4-a76b-bbac81e63eaa"
COLLECTIONS_ID = "unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd"

REUSE = {
    "Library administration and organization": OPERATIONS_ID,
    "Library buildings. Library architecture": PHYSICAL_PLANT_ID,
    "Personnel": PERSONNEL_ID,
    "Information services. Information centers": INFORMATION_SERVICES_ID,
    "The collections. The books": COLLECTIONS_ID,
}
REUSE_SELECTOR_ORDINAL = {
    "Library administration and organization": 0,
    "Library buildings. Library architecture": 0,
    "Personnel": 0,
    "Information services. Information centers": 1,
    "The collections. The books": 25,
}

GROUPS = [
    ("foundations-professional-study", "Foundations & Professional Study", {
        "General works", "Juvenile works", "Bibliography", "Handbooks, manuals, etc.", "Library education. Training",
        "Research. Authorship", "Statistical methods", "Charts, diagrams, etc.", "Periodicals", "Comparative librarianship",
        "International librarianship", "Congresses", "Collected works (nonserial)", "Library humor, anecdotes, etc.",
    }),
    ("organization-information-systems", "Organization & Information Systems", {
        "Information organization", "Information storage and retrieval systems", "Library cooperation and coordination",
        "Library associations", "Information services. Information centers", "Library information networks", "Automation",
        "Library communication systems", "Reproduction of library materials. Storage media of library materials",
    }),
    ("administration-facilities-governance", "Administration, Facilities & Governance", {
        "Library administration and organization", "Library service agencies", "Library buildings. Library architecture",
        "Trustees. Library boards, committees, etc.", "Personnel", "Finance", "Insurance. Risk management", "Supplies",
        "Shelving. Bookstacks", "Branches. Bookmobiles", "Regulations", "Hours of opening. Sunday opening",
        "Endowments. Bequests",
    }),
    ("collections-access", "Collections, Access & Public Services", {
        "The collections. The books", "Aids. Guides", "Public services. Reference services", "Circulation. Loans",
        "Charging systems", "Forms, blanks, etc. used in libraries", "Library extension. Library commissions. Traveling libraries, etc.",
        "Audio-visual library service", "Exhibitions of books, etc., in libraries",
    }),
    ("community-outreach", "Community & Outreach", {
        "Library exhibits", "Libraries and metropolitan areas", "Libraries and rural areas",
        "Public relations. Advertising and marketing. Publicity",
        "Libraries and community. Libraries and society. Social aspects of libraries",
        "Libraries and new literates. Libraries and the illiterate", "Libraries and publishing. Libraries and bookselling",
        "Libraries and museums",
    }),
    ("education-user-groups", "Education & User Groups", {
        "Libraries and schools. Libraries and colleges", "Children's libraries. Children's departments in public libraries, etc.",
        "Libraries and teenagers. Young adults' libraries", "Libraries and students", "Libraries and home schooling",
        "Libraries and adult education", "Libraries and distance education",
    }),
]

LABEL_OVERRIDES = {
    "Aids. Guides": "Aids & Guides",
    "Audio-visual library service": "Audiovisual Library Services",
    "Branches. Bookmobiles": "Branches & Bookmobiles",
    "Charging systems": "Library Charging Systems",
    "Children's libraries. Children's departments in public libraries, etc.": "Children's Libraries & Departments",
    "Circulation. Loans": "Circulation & Loans",
    "Collected works (nonserial)": "Collected Works",
    "Forms, blanks, etc. used in libraries": "Library Forms & Records",
    "Handbooks, manuals, etc.": "Handbooks & Manuals",
    "Hours of opening. Sunday opening": "Opening Hours",
    "Insurance. Risk management": "Insurance & Risk Management",
    "Library administration and organization": "Library & Archive Operations",
    "Library buildings. Library architecture": "Library Buildings & Physical Plant",
    "Library education. Training": "Library Education & Training",
    "Library extension. Library commissions. Traveling libraries, etc.": "Library Extension & Traveling Libraries",
    "Library humor, anecdotes, etc.": "Library Humor & Anecdotes",
    "Personnel": "Library & Archive Personnel Management",
    "Public relations. Advertising and marketing. Publicity": "Public Relations & Marketing",
    "Public services. Reference services": "Public & Reference Services",
    "Reproduction of library materials. Storage media of library materials": "Reproduction & Storage Media",
    "Research. Authorship": "Research & Authorship",
    "Shelving. Bookstacks": "Shelving & Bookstacks",
    "The collections. The books": "Library Collections & Materials",
    "Trustees. Library boards, committees, etc.": "Trustees, Boards & Committees",
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
        WHERE h.label = 'Library science. Information science' AND next_h.record_id IS NULL
          AND cr.caption <> h.label AND span.start_number GLOB 'Z*' AND cr.caption NOT LIKE '%A-Z%'
    """
    with sqlite3.connect(snapshot) as connection:
        return connection.execute(query).fetchall()


def render(records: list[tuple[str, str, str]]) -> str:
    if len(records) != 60:
        raise SystemExit(f"expected 60 semantic Library science records, found {len(records)}")
    assigned = {caption: gid for gid, _, captions in GROUPS for caption in captions}
    actual = {caption for caption, _, _ in records}
    if set(assigned) != actual:
        raise SystemExit(f"group assignments differ: missing={actual - set(assigned)!r}, stale={set(assigned) - actual!r}")

    group_id = lambda gid: f"bokheim:library-science:{gid}"
    topic_id = lambda caption: REUSE.get(caption, f"bokheim:library-science:{slug(caption)}")
    rows = {caption: (start, end) for caption, start, end in records}

    lines = [
        "-- Remove the redundant Libraries / Library science route, reconcile existing",
        "-- DDC and LCC concepts, and add the semantic immediate Z subdivisions.",
        "-- Two alphabetic A-Z filing guides are intentionally excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-library-information-science-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        f"UPDATE concept SET preferred_label='Library & Information Science' WHERE concept_id={sql(LIBRARY_SCIENCE_ID)};",
        f"UPDATE concept SET preferred_label='Libraries & Collections' WHERE concept_id={sql(LIBRARIES_ID)};",
        f"UPDATE concept SET preferred_label='Library Buildings & Physical Plant' WHERE concept_id={sql(PHYSICAL_PLANT_ID)};",
        f"UPDATE concept SET preferred_label='Library & Archive Personnel Management' WHERE concept_id={sql(PERSONNEL_ID)};",
        f"UPDATE concept SET preferred_label='Library & Archive Operations' WHERE concept_id={sql(OPERATIONS_ID)};",
        f"UPDATE concept SET preferred_label='Library Collections & Materials' WHERE concept_id={sql(COLLECTIONS_ID)};", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts = [(group_id(gid), glabel) for gid, glabel, _ in GROUPS]
    concepts += [(topic_id(caption), display_label(caption)) for caption, _, _ in records if caption not in REUSE]
    lines += [f"    ({sql(cid)}, {sql(label)}){',' if i + 1 < len(concepts) else ';'}" for i, (cid, label) in enumerate(concepts)]

    lines += ["", "-- Replace the redundant route and place the existing DDC concepts in semantic branches.",
              f"DELETE FROM concept_parent WHERE concept_id={sql(LIBRARY_SCIENCE_ID)} AND parent_concept_id={sql(LIBRARIES_ID)};",
              f"INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES ({sql(LIBRARY_SCIENCE_ID)}, {sql(DOMAIN_ID)}, 0, 20);",
              f"UPDATE concept_parent SET parent_concept_id={sql(group_id('organization-information-systems'))} WHERE concept_id={sql(RELATIONSHIPS_ID)} AND parent_concept_id={sql(DOMAIN_ID)};",
              f"UPDATE concept_parent SET parent_concept_id={sql(group_id('administration-facilities-governance'))} WHERE concept_id IN ({sql(PHYSICAL_PLANT_ID)}, {sql(PERSONNEL_ID)}, {sql(OPERATIONS_ID)}) AND parent_concept_id={sql(DOMAIN_ID)};",
              f"UPDATE concept_parent SET parent_concept_id={sql(group_id('collections-access'))} WHERE concept_id={sql(READING_MEDIA_ID)} AND parent_concept_id={sql(DOMAIN_ID)};",
              f"UPDATE concept_parent SET parent_concept_id={sql(LIBRARIES_ID)} WHERE concept_id IN ({sql(SPECIFIC_LIBRARIES_ID)}, {sql(GENERAL_LIBRARIES_ID)}) AND parent_concept_id={sql(DOMAIN_ID)};", "",
              "INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES"]
    edges = []
    for gi, (gid, _, captions) in enumerate(GROUPS, 1):
        gid_full = group_id(gid)
        edges.append((gid_full, LIBRARY_SCIENCE_ID, 0, gi * 10))
        for ti, caption in enumerate(sorted(captions, key=lambda value: rows[value][0]), 1):
            if caption in {"Library administration and organization", "Library buildings. Library architecture", "Personnel"}:
                continue
            ordinal = 1 if caption in REUSE else 0
            edges.append((topic_id(caption), gid_full, ordinal, ti * 10))
    lines += [f"    ({sql(cid)}, {sql(pid)}, {ordinal}, {order}){',' if i + 1 < len(edges) else ';'}" for i, (cid, pid, ordinal, order) in enumerate(edges)]

    lines += ["", "-- Priority 4 supersedes the existing priority-3 outline ranges.",
              "INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES"]
    selectors = []
    for caption, start, end in records:
        ordinal = REUSE_SELECTOR_ORDINAL.get(caption, 0)
        selectors.append((topic_id(caption), f"{start}..{end}@4", ordinal))
    lines += [f"    ({sql(cid)}, 'lcc', {sql(selector)}, {ordinal}){',' if i + 1 < len(selectors) else ';'}" for i, (cid, selector, ordinal) in enumerate(selectors)]

    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES"]
    labels = [(topic_id(caption), caption, 0) for caption, _, _ in records]
    lines += [f"    ({sql(cid)}, {sql(label)}, {ordinal}){',' if i + 1 < len(labels) else ';'}" for i, (cid, label, ordinal) in enumerate(labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_library_information_science_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")


if __name__ == "__main__":
    main()
