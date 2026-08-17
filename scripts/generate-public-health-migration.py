#!/usr/bin/env python3
"""Generate the curated Public Health LCC expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path

PARENT_ID = "unified:3cee8352-db8c-536e-8b96-8b0ab9b29c71"
DUPLICATE_ID = "unified:a41dd950-86e1-557f-b749-faf234640c50"
REUSE = {
    "Health risk assessment": "unified:cc1c53d4-6866-5c80-91cd-64415c80965c",
    "Environmental health": "unified:6c04e0c7-7321-5430-a5b3-12b0218d4e84",
    "Emergency medical services": "unified:18f47110-9460-5bb2-b07c-519baf655407",
    "Epidemics. Epidemiology": "unified:5e6d7df6-4278-5bf6-b4c2-040ea8d80045",
}
GROUPS = [
    ("reference-history-study", "Reference, History & Study", {
        "Periodicals. Societies. Serials", "Congresses", "Collected works (nonserial)",
        "Nomenclature. Terminology. Abbreviations", "Dictionaries and encyclopedias",
        "Communication in public health", "Directories", "History", "Biography", "General works", "General special",
        "Handbooks, manuals, etc.", "Problems, exercises, examinations", "Outlines, syllabi, etc.", "Popular works",
        "Juvenile works", "Addresses, essays, lectures", "Museums. Exhibitions", "Study and teaching",
        "Research. Experimentation", "Public health as a profession",
    }),
    ("foundations-ethics-practice", "Foundations, Ethics & Practice", {
        "Moral and ethical aspects", "Health risk assessment", "Medical screening", "Health promotion",
        "Primary health care", "Public health laboratories, institutes, etc.",
    }),
    ("global-population-health", "Global & Population Health", {
        "World health. International cooperation", "Developing countries", "By region or country",
        "By ethnic group, etc.", "By age group, class, etc.",
    }),
    ("environmental-community-health", "Environmental & Community Health", {
        "Environmental health", "Food and food supply in relation to public health",
        "Parks, recreation areas, camp grounds, etc.", "Public baths, comfort stations, etc.",
        "Public health aspects of transportation", "Public buildings", "Barber shops. Beauty shops", "Hotels",
        "Disposal of the dead", "Housing and public health", "Household hygiene",
        "Rural health and hygiene. Rural health services",
    }),
    ("prevention-immunity-disease", "Prevention, Immunity & Disease", {
        "Immunity and immunization in relation to public health", "Transmission of disease",
        "Communicable diseases and public health", "Acute diseases and public health",
        "Chronic and noninfectious diseases and public health", "Epidemics. Epidemiology",
        "Quarantine (Land, maritime, and air)", "Disinfection. Fumigation. Sterilization. Decontamination",
    }),
    ("services-emergency-preparedness", "Health Services & Emergency Preparedness", {
        "Home health care services", "Emergency medical services", "War and public health",
    }),
    ("personal-mental-health", "Personal & Mental Health", {
        "Personal health and hygiene", "Mental health. Mental illness prevention",
    }),
]
LABEL_OVERRIDES = {
    "Addresses, essays, lectures": "Addresses, Essays & Lectures",
    "Barber shops. Beauty shops": "Barber & Beauty Shops",
    "By age group, class, etc.": "Public Health by Age Group & Class",
    "By ethnic group, etc.": "Public Health by Ethnic Group",
    "By region or country": "Public Health by Region or Country",
    "Chronic and noninfectious diseases and public health": "Chronic & Noninfectious Diseases in Public Health",
    "Collected works (nonserial)": "Collected Works",
    "Dictionaries and encyclopedias": "Dictionaries & Encyclopedias",
    "Disinfection. Fumigation. Sterilization. Decontamination": "Disinfection, Fumigation, Sterilization & Decontamination",
    "Epidemics. Epidemiology": "Epidemiology",
    "Food and food supply in relation to public health": "Food Supply & Public Health",
    "General special": "Special Topics (General)",
    "Handbooks, manuals, etc.": "Handbooks & Manuals",
    "Immunity and immunization in relation to public health": "Immunity & Immunization in Public Health",
    "Mental health. Mental illness prevention": "Mental Health & Mental Illness Prevention",
    "Museums. Exhibitions": "Museums & Exhibitions",
    "Nomenclature. Terminology. Abbreviations": "Terminology & Abbreviations",
    "Parks, recreation areas, camp grounds, etc.": "Parks, Recreation Areas & Campgrounds",
    "Periodicals. Societies. Serials": "Periodicals, Societies & Serials",
    "Problems, exercises, examinations": "Problems, Exercises & Examinations",
    "Public baths, comfort stations, etc.": "Public Baths & Comfort Stations",
    "Public health laboratories, institutes, etc.": "Public Health Laboratories & Institutes",
    "Quarantine (Land, maritime, and air)": "Land, Maritime & Air Quarantine",
    "Research. Experimentation": "Research & Experimentation",
    "Rural health and hygiene. Rural health services": "Rural Health, Hygiene & Services",
    "War and public health": "War & Public Health",
    "World health. International cooperation": "World Health & International Cooperation",
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

def lcc_key(number: str) -> tuple[float, str]:
    match = re.match(r"RA([0-9]+(?:\.[0-9]+)?)(.*)", number)
    if not match:
        raise SystemExit(f"unexpected LCC number: {number}")
    return float(match.group(1)), match.group(2)

def load_records(snapshot: Path) -> list[tuple[str, str, str]]:
    query = """
        SELECT cr.caption, span.start_number, coalesce(span.end_number, span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h ON h.record_id=cr.record_id
        LEFT JOIN caption_hierarchy nh ON nh.record_id=h.record_id AND nh.ordinal=h.ordinal+1
        JOIN classification_span span ON span.record_id=cr.record_id AND span.ordinal=0
        WHERE h.label='Public health. Hygiene. Preventive medicine' AND nh.record_id IS NULL
          AND cr.caption<>h.label AND cr.caption NOT LIKE '%A-Z%'
    """
    with sqlite3.connect(snapshot) as connection:
        records = connection.execute(query).fetchall()
    return sorted(records, key=lambda row: lcc_key(row[1]))

def render(records: list[tuple[str, str, str]]) -> str:
    if len(records) != 57:
        raise SystemExit(f"expected 57 semantic public-health records, found {len(records)}")
    assigned = {caption for _, _, captions in GROUPS for caption in captions}
    actual = {caption for caption, _, _ in records}
    if assigned != actual:
        raise SystemExit(f"group assignments differ: missing={actual-assigned!r}, stale={assigned-actual!r}")
    group_id = lambda gid: f"bokheim:public-health:{gid}"
    topic_id = lambda caption: REUSE.get(caption, f"bokheim:public-health:{slug(caption)}")
    rows = {caption: (start, end) for caption, start, end in records}
    lines = [
        "-- Reconcile the BISAC and LCC Public Health concepts and add the semantic immediate RA subdivisions.",
        "-- The 'Other subjects of public health, A-Z' filing guide is intentionally excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-public-health-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        "-- Move the BISAC route and selector to the LCC-backed canonical concept.",
        f"INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT {sql(PARENT_ID)},parent_concept_id,1,display_order FROM concept_parent WHERE concept_id={sql(DUPLICATE_ID)};",
        f"INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES ({sql(PARENT_ID)},'bisac','MED078000',0);",
        f"DELETE FROM concept_parent WHERE concept_id={sql(DUPLICATE_ID)} OR parent_concept_id={sql(DUPLICATE_ID)};",
        f"DELETE FROM concept WHERE concept_id={sql(DUPLICATE_ID)};",
        f"UPDATE concept SET preferred_label='Public Health' WHERE concept_id={sql(PARENT_ID)};", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts = [(group_id(gid), label) for gid, label, _ in GROUPS]
    concepts += [(topic_id(c), display_label(c)) for c, _, _ in records if c not in REUSE]
    lines += [f"    ({sql(cid)}, {sql(label)}){',' if i+1<len(concepts) else ';'}" for i, (cid, label) in enumerate(concepts)]
    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES"]
    edges = []
    for gi, (gid, _, captions) in enumerate(GROUPS, 1):
        full_gid = group_id(gid)
        edges.append((full_gid, PARENT_ID, 0, gi * 10))
        for ti, caption in enumerate(sorted(captions, key=lambda c: lcc_key(rows[c][0])), 1):
            edges.append((topic_id(caption), full_gid, 1 if caption in REUSE else 0, ti * 10))
    lines += [f"    ({sql(cid)}, {sql(pid)}, {ordinal}, {order}){',' if i+1<len(edges) else ';'}" for i, (cid,pid,ordinal,order) in enumerate(edges)]
    lines += ["", "-- Priority 4 supersedes the existing priority-3 outline ranges.",
              "INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES"]
    selectors = [(topic_id(c), f"{start}..{end}@4", 0) for c,start,end in records]
    lines += [f"    ({sql(cid)},'lcc',{sql(selector)},{ordinal}){',' if i+1<len(selectors) else ';'}" for i,(cid,selector,ordinal) in enumerate(selectors)]
    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES"]
    labels = [(topic_id(c), c, 0) for c,_,_ in records]
    lines += [f"    ({sql(cid)},{sql(label)},{ordinal}){',' if i+1<len(labels) else ';'}" for i,(cid,label,ordinal) in enumerate(labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("client/library-model/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("client/library-model/data/migrations/20260817_public_health_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")

if __name__ == "__main__":
    main()
