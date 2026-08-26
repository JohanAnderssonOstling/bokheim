#!/usr/bin/env python3
"""Generate the curated Management & Industrial Management LCC expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path

PARENT_ID = "unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b"
REUSE = {
    "Leadership": "unified:58a5cf9c-e988-541d-a313-e8f15ddd251d",
    "Organizational behavior. Corporate culture": "unified:7c85dadb-88b6-5af7-83d3-1c41aa37add8",
    "Public relations. Industrial publicity": "unified:d7d69b65-fd90-59a9-b6bf-978139d89542",
    "Quality control": "unified:4258aa53-e461-55aa-93f1-7676e0383420",
    "Total quality management. Benchmarking": "unified:f87a1bb5-fc8b-5296-8267-859126d37826",
    "Valuation": "unified:2cf4b310-cf52-5477-9cd8-5c003cb185cd",
}
GROUPS = [
    ("reference-history-study", "Reference, History & Study", {
        "Periodicals. Societies. Serials", "Congresses", "Collected works (nonserial)",
        "Dictionaries. Encyclopedias", "Terminology. Abbreviations. Notation",
        "Theory. Method. Relation to other subjects",
        "Communication in management. Communication in organizations. Communication of information",
        "Study and teaching. Research", "History", "General works", "General special",
        "Handbooks, manuals, etc.", "Charts, diagrams, etc.",
    }),
    ("leadership-organization-people", "Leadership, Organization & People", {
        "Executives. Executive ability", "Bureaucracy", "Conflict management",
        "Delegation of authority. Decentralization. Span of control", "Division of labor. Specialization",
        "Leadership", "Followership", "Management committees", "Matrix organization",
        "Negotiation. Persuasion", "Organizational behavior. Corporate culture",
        "Organizational change. Organizational development. Corporate turnarounds",
        "Organizational learning", "Downsizing", "Work groups. Teams in the workplace",
    }),
    ("strategy-intelligence-innovation", "Strategy, Intelligence & Innovation", {
        "Business intelligence. Trade secrets", "Competition", "Technological innovations", "Automation",
        "Location of industry", "Intellectual work. Intellectual capital",
    }),
    ("operations-logistics-quality", "Operations, Logistics & Quality", {
        "Business logistics", "Costs. Industrial costs", "Industrial productivity", "Reengineering",
        "Organizational effectiveness", "Quality control", "Standardization. Simplification. Waste",
        "Support services", "Total quality management. Benchmarking", "Workflow",
        "Management of special enterprises",
    }),
    ("finance-risk-responsibility", "Finance, Risk & Responsibility", {
        "Capital. Capital investments", "Crisis management. Emergency management",
        "Public relations. Industrial publicity", "Social responsibility of business. Social entrepreneurship",
        "Revenue management", "Risk management. Risk in industry", "Security measures", "Valuation",
    }),
]
LABEL_OVERRIDES = {
    "Business intelligence. Trade secrets": "Business Intelligence & Trade Secrets",
    "Capital. Capital investments": "Capital & Capital Investments",
    "Charts, diagrams, etc.": "Charts & Diagrams",
    "Collected works (nonserial)": "Collected Works",
    "Communication in management. Communication in organizations. Communication of information": "Management & Organizational Communication",
    "Costs. Industrial costs": "Costs & Industrial Costs",
    "Crisis management. Emergency management": "Crisis & Emergency Management",
    "Delegation of authority. Decentralization. Span of control": "Delegation, Decentralization & Span of Control",
    "Dictionaries. Encyclopedias": "Dictionaries & Encyclopedias",
    "Division of labor. Specialization": "Division of Labor & Specialization",
    "Executives. Executive ability": "Executives & Executive Ability",
    "General special": "Special Topics (General)",
    "Handbooks, manuals, etc.": "Handbooks & Manuals",
    "Intellectual work. Intellectual capital": "Intellectual Work & Capital",
    "Negotiation. Persuasion": "Negotiation & Persuasion",
    "Organizational behavior. Corporate culture": "Organizational Behavior & Corporate Culture",
    "Organizational change. Organizational development. Corporate turnarounds": "Organizational Change, Development & Turnarounds",
    "Periodicals. Societies. Serials": "Periodicals, Societies & Serials",
    "Public relations. Industrial publicity": "Public Relations & Industrial Publicity",
    "Risk management. Risk in industry": "Risk Management in Industry",
    "Social responsibility of business. Social entrepreneurship": "Business Social Responsibility & Social Entrepreneurship",
    "Standardization. Simplification. Waste": "Standardization, Simplification & Waste Reduction",
    "Study and teaching. Research": "Study, Teaching & Research",
    "Terminology. Abbreviations. Notation": "Terminology, Abbreviations & Notation",
    "Theory. Method. Relation to other subjects": "Management Theory, Methods & Relations to Other Subjects",
    "Total quality management. Benchmarking": "Total Quality Management & Benchmarking",
    "Work groups. Teams in the workplace": "Work Groups & Workplace Teams",
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
        SELECT cr.caption, span.start_number, coalesce(span.end_number,span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h ON h.record_id=cr.record_id
        LEFT JOIN caption_hierarchy nh ON nh.record_id=h.record_id AND nh.ordinal=h.ordinal+1
        JOIN classification_span span ON span.record_id=cr.record_id AND span.ordinal=0
        WHERE h.label='Management. Industrial management' AND nh.record_id IS NULL
          AND cr.caption<>h.label AND cr.caption NOT LIKE '%A-Z%'
          AND span.start_number GLOB 'HD*'
    """
    with sqlite3.connect(snapshot) as connection:
        records = connection.execute(query).fetchall()
    return sorted(records, key=lambda row: lcc_key(row[1]))

def render(records: list[tuple[str, str, str]]) -> str:
    if len(records) != 53:
        raise SystemExit(f"expected 53 semantic management records, found {len(records)}")
    assigned = {caption for _,_,captions in GROUPS for caption in captions}
    actual = {caption for caption,_,_ in records}
    if assigned != actual:
        raise SystemExit(f"group assignments differ: missing={actual-assigned!r}, stale={assigned-actual!r}")
    group_id = lambda gid: f"bokheim:industrial-management:{gid}"
    topic_id = lambda caption: REUSE.get(caption, f"bokheim:industrial-management:{slug(caption)}")
    rows = {caption:(start,end) for caption,start,end in records}
    lines = [
        "-- Add the semantic immediate Management & Industrial Management HD subdivisions.",
        "-- The two alphabetic A-Z filing guides are intentionally excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-industrial-management-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        f"UPDATE concept SET preferred_label='Management & Industrial Management' WHERE concept_id={sql(PARENT_ID)};", "",
        "INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES",
    ]
    concepts = [(group_id(gid),label) for gid,label,_ in GROUPS]
    concepts += [(topic_id(c),display_label(c)) for c,_,_ in records if c not in REUSE]
    lines += [f"    ({sql(cid)},{sql(label)}){',' if i+1<len(concepts) else ';'}" for i,(cid,label) in enumerate(concepts)]
    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES"]
    edges = []
    for gi,(gid,_,captions) in enumerate(GROUPS,1):
        full_gid = group_id(gid)
        edges.append((full_gid,PARENT_ID,0,gi*10))
        for ti,caption in enumerate(sorted(captions,key=lambda c:lcc_key(rows[c][0])),1):
            edges.append((topic_id(caption),full_gid,1 if caption in REUSE else 0,ti*10))
    lines += [f"    ({sql(cid)},{sql(pid)},{ordinal},{order}){',' if i+1<len(edges) else ';'}" for i,(cid,pid,ordinal,order) in enumerate(edges)]
    lines += ["", "-- Priority 4 supersedes the existing priority-3 outline ranges.",
              "INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES"]
    selectors = [(topic_id(c),f"{start}..{end}@4",0) for c,start,end in records]
    lines += [f"    ({sql(cid)},'lcc',{sql(selector)},{ordinal}){',' if i+1<len(selectors) else ';'}" for i,(cid,selector,ordinal) in enumerate(selectors)]
    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES"]
    labels = [(topic_id(c),c,0) for c,_,_ in records]
    lines += [f"    ({sql(cid)},{sql(label)},{ordinal}){',' if i+1<len(labels) else ';'}" for i,(cid,label,ordinal) in enumerate(labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot",type=Path,default=Path("shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output",type=Path,default=Path("shared/subject-projection/data/migrations/20260817_industrial_management_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)),encoding="utf-8")

if __name__ == "__main__":
    main()
