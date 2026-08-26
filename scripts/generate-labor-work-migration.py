#!/usr/bin/env python3
"""Generate the curated Labor, Work & Working Class LCC expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path

PARENT_ID = "unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab"
REUSE = {
    "Wages": "unified:672c3d91-fbc8-51a9-9b82-c87059d23048",
    "Trade unions. Labor unions. Workers' associations": "unified:f1b37b62-0d06-5d7c-9a9b-20b08c4f57f3",
    "Industrial relations": "unified:9fc3f663-b30e-5bb3-bb87-d752fe6facd1",
}
GROUPS = [
    ("reference-history-study", "Reference, History & Study", {
        "Periodicals. Societies. Serials", "Congresses", "International directories", "Collected works",
        "Museums. Exhibitions", "Study and teaching. Research", "Statistics", "Labor bureaus. Departments of labor, etc.",
        "Dictionaries. Encyclopedias", "History", "General works", "Syllabi, outlines", "Juvenile works",
        "Theory. Method. Relation to other subjects",
    }),
    ("work-employment-markets", "Work, Employment & Labor Markets", {
        "Labor systems", "Free choice of employment. Freedom of labor. Discrimination in employment", "Work and family",
        "Fatigue", "Leisure and work", "Human capital", "Labor market. Labor supply. Labor demand",
        "Employment agencies. State employment bureaus. Labor exchanges", "Self-employed",
        "Professions (General). Professional employees",
    }),
    ("wages-hours-conditions", "Wages, Hours & Working Conditions", {
        "Wages", "Hours of labor", "Cost and standard of living",
        "Vocational rehabilitation. Employment of people with disabilities", "Industrial hygiene. Industrial welfare",
    }),
    ("relations-disputes-participation", "Labor Relations, Disputes & Participation", {
        "Labor disputes. Strikes and lockouts", "Industrial arbitration. Mediation and conciliation",
        "Employee participation in management. Employee ownership. Industrial democracy. Works councils", "Industrial relations",
    }),
    ("unions-employers", "Unions & Employer Associations", {
        "Trade unions. Labor unions. Workers' associations", "Employers' associations",
    }),
    ("employment-types-workers", "Employment Types & Classes of Labor", {
        "Temporary employment", "Supplementary employment. Second jobs", "Seasonal labor. Migrant labor",
        "Precarious employment", "Under-the-table employment", "Classes of labor",
    }),
    ("social-protection", "Social Protection & Pensions", {"Social insurance. Social security. Pensions"}),
    ("technology-society-religion", "Labor, Technology, Society & Religion", {
        "Machinery in the workplace. Technological unemployment. Effect of technological innovations on employees",
        "Labor and the trusts", "Labor and international business enterprises. International business enterprise employees",
        "Church and labor", "Judaism and labor", "Islam and labor", "Labor and the intellectuals",
        "Industrial sociology. Social conditions of labor",
    }),
    ("policy-politics-culture", "Labor Policy, Politics & Culture", {
        "Ethics. Dignity of labor. Work ethic", "Labor in time of war", "Workers' travel programs", "Labor day. May day",
        "Labor policy. Labor and the state", "Labor in politics. Political activity of the working class", "By region or country",
    }),
]
LABEL_OVERRIDES = {
    "By region or country": "Labor by Region or Country",
    "Cost and standard of living": "Cost & Standard of Living",
    "Dictionaries. Encyclopedias": "Dictionaries & Encyclopedias",
    "Employee participation in management. Employee ownership. Industrial democracy. Works councils": "Employee Participation, Ownership & Industrial Democracy",
    "Employment agencies. State employment bureaus. Labor exchanges": "Employment Agencies & Labor Exchanges",
    "Ethics. Dignity of labor. Work ethic": "Labor Ethics, Dignity & Work Ethic",
    "Free choice of employment. Freedom of labor. Discrimination in employment": "Freedom & Discrimination in Employment",
    "Industrial arbitration. Mediation and conciliation": "Industrial Arbitration, Mediation & Conciliation",
    "Industrial hygiene. Industrial welfare": "Industrial Hygiene & Welfare",
    "Industrial sociology. Social conditions of labor": "Industrial Sociology & Labor Conditions",
    "Labor and international business enterprises. International business enterprise employees": "Labor & International Business",
    "Labor bureaus. Departments of labor, etc.": "Labor Bureaus & Departments",
    "Labor day. May day": "Labor Day & May Day",
    "Labor disputes. Strikes and lockouts": "Labor Disputes, Strikes & Lockouts",
    "Labor in politics. Political activity of the working class": "Labor Politics & Working-Class Political Activity",
    "Labor market. Labor supply. Labor demand": "Labor Markets, Supply & Demand",
    "Labor policy. Labor and the state": "Labor Policy & the State",
    "Machinery in the workplace. Technological unemployment. Effect of technological innovations on employees": "Workplace Technology & Technological Unemployment",
    "Museums. Exhibitions": "Museums & Exhibitions",
    "Periodicals. Societies. Serials": "Periodicals, Societies & Serials",
    "Professions (General). Professional employees": "Professions & Professional Employees",
    "Seasonal labor. Migrant labor": "Seasonal & Migrant Labor",
    "Social insurance. Social security. Pensions": "Social Insurance, Security & Pensions",
    "Study and teaching. Research": "Study, Teaching & Research",
    "Supplementary employment. Second jobs": "Supplementary Employment & Second Jobs",
    "Theory. Method. Relation to other subjects": "Labor Theory, Methods & Relations to Other Subjects",
    "Trade unions. Labor unions. Workers' associations": "Trade Unions, Labor Unions & Workers' Associations",
    "Vocational rehabilitation. Employment of people with disabilities": "Vocational Rehabilitation & Employment of People with Disabilities",
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
    match = re.match(r"HD([0-9]+(?:\.[0-9]+)?)(.*)", number)
    if not match:
        raise SystemExit(f"unexpected LCC number: {number}")
    return float(match.group(1)), match.group(2)

def load_records(snapshot: Path) -> list[tuple[str, str, str]]:
    query = """
        SELECT cr.caption, span.start_number, coalesce(span.end_number, span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h ON h.record_id = cr.record_id
        LEFT JOIN caption_hierarchy nh ON nh.record_id = h.record_id AND nh.ordinal = h.ordinal + 1
        JOIN classification_span span ON span.record_id = cr.record_id AND span.ordinal = 0
        WHERE h.label = 'Labor. Work. Working class' AND nh.record_id IS NULL
          AND cr.caption <> h.label AND cr.caption NOT LIKE '%A-Z%'
    """
    with sqlite3.connect(snapshot) as connection:
        records = connection.execute(query).fetchall()
    return sorted(records, key=lambda row: lcc_key(row[1]))

def render(records: list[tuple[str, str, str]]) -> str:
    if len(records) != 57:
        raise SystemExit(f"expected 57 semantic labor records, found {len(records)}")
    assigned = {caption for _, _, captions in GROUPS for caption in captions}
    actual = {caption for caption, _, _ in records}
    if assigned != actual:
        raise SystemExit(f"group assignments differ: missing={actual-assigned!r}, stale={assigned-actual!r}")
    group_id = lambda gid: f"bokheim:labor:{gid}"
    topic_id = lambda caption: REUSE.get(caption, f"bokheim:labor:{slug(caption)}")
    rows = {caption: (start, end) for caption, start, end in records}
    lines = [
        "-- Add the semantic immediate Labor, Work & Working Class HD subdivisions.",
        "-- The 'By industry or trade, A-Z' filing guide is intentionally excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-labor-work-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        f"UPDATE concept SET preferred_label='Labor, Work & Working Class' WHERE concept_id={sql(PARENT_ID)};", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts = [(group_id(gid), label) for gid, label, _ in GROUPS]
    concepts += [(topic_id(c), display_label(c)) for c, _, _ in records if c not in REUSE]
    lines += [f"    ({sql(cid)}, {sql(label)}){',' if i+1<len(concepts) else ';'}" for i, (cid, label) in enumerate(concepts)]
    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES"]
    edges = []
    for gi, (gid, _, captions) in enumerate(GROUPS, 1):
        full_gid = group_id(gid)
        edges.append((full_gid, PARENT_ID, 0, gi * 10))
        for ti, caption in enumerate(sorted(captions, key=lambda c: lcc_key(rows[c][0])), 1):
            edges.append((topic_id(caption), full_gid, 1 if caption in REUSE else 0, ti * 10))
    lines += [f"    ({sql(cid)}, {sql(pid)}, {ordinal}, {order}){',' if i+1<len(edges) else ';'}" for i, (cid, pid, ordinal, order) in enumerate(edges)]
    lines += ["", "-- Priority 4 supersedes the existing priority-3 outline ranges.",
              "INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES"]
    selectors = [(topic_id(c), f"{start}..{end}@4", 0) for c, start, end in records]
    lines += [f"    ({sql(cid)}, 'lcc', {sql(selector)}, {ordinal}){',' if i+1<len(selectors) else ';'}" for i, (cid, selector, ordinal) in enumerate(selectors)]
    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES"]
    labels = [(topic_id(c), c, 0) for c, _, _ in records]
    lines += [f"    ({sql(cid)}, {sql(label)}, {ordinal}){',' if i+1<len(labels) else ';'}" for i, (cid, label, ordinal) in enumerate(labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_labor_work_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")

if __name__ == "__main__":
    main()
