#!/usr/bin/env python3
"""Generate the curated expansion of the LCC Other Protestant denominations leaf."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path


PARENT_ID = "unified:19fed5d0-fb6d-58d4-b6c3-4541e89ae5b1"

GROUPS = [
    ("historic-traditions", "Historic Denominational Traditions"),
    ("reformed-congregational", "Reformed & Congregational Traditions"),
    ("brethren-peace", "Brethren, Friends & Peace Churches"),
    ("holiness-pentecostal", "Holiness, Pentecostal & Apostolic Churches"),
    ("evangelical-free", "Evangelical, Free & Covenant Churches"),
    ("united-regional", "United, Regional & Ecumenical Churches"),
    ("restoration-post-protestant", "Restorationist & Post-Protestant Movements"),
    ("independent-movements", "Independent & Other Protestant Movements"),
]

# These concepts already represent the same religious subjects elsewhere in
# the unified Protestant hierarchy. Reuse them instead of creating duplicates.
REUSE = {
    "Baptists": "unified:13e8887d-418a-5989-9954-87f254dc83c2",
    "Friends. Society of Friends. Quakers": "unified:bb612d78-008d-546f-abf0-d9a2757dbe3d",
    "Lutheran churches": "unified:88dc9a38-69a3-5019-9086-820b28c6dd96",
    "Mennonites": "unified:bf1172da-5b27-5cca-97df-13c2de7071ed",
    "Methodism": "unified:5fb2d52c-b4b8-5719-ba39-9e40dec359ce",
    "Pentecostal churches": "unified:25c4a755-9c54-5eb9-8e03-c14b77f4767d",
    "Presbyterianism. Calvinistic Methodism": "unified:b7541e1e-cc12-5971-bed6-4da0ff37d3f8",
    "Shakers. United Society of Believers. Millennial Church": "unified:3dbd80fb-0a35-528a-958e-c56437c9f0c7",
    "United Church of Christ": "unified:1b0023cd-a0e7-557b-b32b-8e3fe74c2a8d",
}

REUSE_SELECTOR_ORDINAL = {
    "Baptists": 11,
    "Friends. Society of Friends. Quakers": 0,
    "Lutheran churches": 0,
    "Mennonites": 0,
    "Methodism": 0,
    "Pentecostal churches": 0,
    "Presbyterianism. Calvinistic Methodism": 0,
    "Shakers. United Society of Believers. Millennial Church": 0,
    "United Church of Christ": 0,
}

REUSE_LABEL_ORDINAL = {caption: (1 if caption in {"Baptists", "Methodism"} else 0) for caption in REUSE}

LABEL_OVERRIDES = {
    "Adventists. \"Millerites\"": "Adventists & Millerites",
    "Arminians. Remonstrants. Remonstrantsche Broederschap": "Arminians & Remonstrants",
    "Baptists": "Baptist",
    "Catholic Apostolic Church. Irvingites": "Catholic Apostolic Church & Irvingites",
    "Christadelphians. Brothers of Christ": "Christadelphians",
    "Church of Daniel's Band": "Church of Daniel's Band",
    "Disciples of Christ. Campbellites. Christians": "Disciples of Christ & Campbellites",
    "Divine, Father (the man and his following)": "Father Divine Movement",
    "Dukhobors": "Doukhobors",
    "Friends. Society of Friends. Quakers": "Quaker",
    "German Baptist Brethren. Church of the Brethren. \"Dunkards\" or \"Dunkers\"": "German Baptist Brethren & Church of the Brethren",
    "Mennonites": "Mennonite",
    "Methodism": "Methodist & Related Churches",
    "Millennial Dawnists. Jehovah's Witnesses": "Jehovah's Witnesses & Bible Student Movement",
    "Moravian Church. United Brethren. Unitas Fratrum. Herrnhuters": "Moravian Church & Unitas Fratrum",
    "Mormons. Church of Jesus Christ of Latter-Day Saints": "Latter-day Saint Movement",
    "New Jerusalem Church. New Church. Swedenborgianism": "New Church & Swedenborgianism",
    "Pentecostal churches": "Pentecostal & Charismatic",
    "Plymouth Brethren. Darbyites": "Plymouth Brethren",
    "Presbyterianism. Calvinistic Methodism": "Presbyterian",
    "Reformed or Calvinistic churches": "Reformed & Calvinistic Churches",
    "River Brethren. Brethren in Christ": "River Brethren & Brethren in Christ",
    "Sandemanians (Glassites or Glasites)": "Sandemanians & Glasites",
    "Schwenckfelder Church. Schwenckfeldians": "Schwenckfelder Church",
    "Shakers. United Society of Believers. Millennial Church": "Shaker",
    "Unitarianism": "Unitarianism & Unitarian Churches",
    "Universalism. Universalists": "Universalism & Universalist Churches",
}

HISTORIC = {
    "Adventists. \"Millerites\"", "Baptists", "Christian Church (General Convention of the Christian Church)",
    "Lutheran churches", "Methodism", "Salvation Army",
}
REFORMED = {
    "Arminians. Remonstrants. Remonstrantsche Broederschap", "Christian Reformed Church", "Congregationalism",
    "Evangelical and Reformed Church", "Presbyterianism. Calvinistic Methodism",
    "Protestant Reformed Churches of America", "Puritanism", "Reformed or Calvinistic churches",
    "United Church of Christ",
}
BRETHREN = {
    "Dukhobors", "Friends. Society of Friends. Quakers",
    "German Baptist Brethren. Church of the Brethren. \"Dunkards\" or \"Dunkers\"", "Mennonites",
    "Moravian Church. United Brethren. Unitas Fratrum. Herrnhuters", "Plymouth Brethren. Darbyites",
    "River Brethren. Brethren in Christ", "Sandemanians (Glassites or Glasites)",
    "Schwenckfelder Church. Schwenckfeldians",
}
RESTORATION = {
    "Catholic Apostolic Church. Irvingites", "Christadelphians. Brothers of Christ", "Christian Science",
    "Church of America, which is the Church of God", "Church of American Science", "Divine, Father (the man and his following)",
    "Divine Science Church", "Dowieism. Christian Catholic Church", "Millennial Dawnists. Jehovah's Witnesses",
    "Mormons. Church of Jesus Christ of Latter-Day Saints", "Muckers (Ebelians)", "Muggletonians",
    "New Jerusalem Church. New Church. Swedenborgianism", "Shakers. United Society of Believers. Millennial Church",
    "Unitarianism", "Universal Apostolic Church of Life", "Universalism. Universalists",
}
AFRICAN_OR_REGIONAL_MARKERS = (
    "Africa", "Canada", "Chile", "China", "Curaçao", "Germany", "India", "Indonesia", "Nigeria",
    "Papua", "Philippines", "Rio Grande do Sul", "Swaziland", "Zambia",
)


def group_for(caption: str) -> str:
    if caption in HISTORIC:
        return "historic-traditions"
    if caption in REFORMED:
        return "reformed-congregational"
    if caption in BRETHREN:
        return "brethren-peace"
    if caption in RESTORATION:
        return "restoration-post-protestant"
    if any(marker in caption for marker in AFRICAN_OR_REGIONAL_MARKERS) or caption.startswith(("Deutsche Evangelische", "Evangelische Kirche", "Evangelische (Landes)", "Evangelische Landeskirche", "Protestantse Kerk", "Vereinigte Protestant", "Walloon Church")):
        return "united-regional"
    if any(marker in caption for marker in ("Church of God", "Churches of God", "Holiness", "Pentecostal", "Apostolic", "Church of Christ", "Churches of Christ", "First Born", "Living God")):
        return "holiness-pentecostal"
    if any(marker in caption for marker in ("Evangelical", "Free Congregations", "Free Religious", "Faith Evangelical", "United Brethren", "United Missionary")):
        return "evangelical-free"
    if caption.startswith("United Church"):
        return "united-regional"
    return "independent-movements"


def slug(value: str) -> str:
    value = unicodedata.normalize("NFKD", value).encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def label(caption: str) -> str:
    if caption in LABEL_OVERRIDES:
        return LABEL_OVERRIDES[caption]
    value = caption.replace(". ", ", ").replace(" and ", " & ").rstrip(".").title()
    for word in ("And", "As", "In", "Of", "Or", "The", "To", "Von"):
        value = re.sub(rf"(?<= )({word})(?= )", word.lower(), value)
    return value


def load_records(snapshot: Path) -> list[tuple[str, str, str]]:
    query = """
        SELECT cr.caption, span.start_number, coalesce(span.end_number, span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h ON h.record_id = cr.record_id
        LEFT JOIN caption_hierarchy next_h ON next_h.record_id = h.record_id AND next_h.ordinal = h.ordinal + 1
        JOIN classification_span span ON span.record_id = cr.record_id AND span.ordinal = 0
        WHERE h.label = 'Other Protestant denominations' AND next_h.record_id IS NULL
          AND cr.caption <> h.label AND span.start_number GLOB 'BX*' AND cr.caption NOT LIKE '% - %'
        ORDER BY CAST(substr(span.start_number, length(rtrim(span.start_number, '0123456789.')) + 1) AS REAL), span.start_number
    """
    with sqlite3.connect(snapshot) as connection:
        return connection.execute(query).fetchall()


def render(records: list[tuple[str, str, str]]) -> str:
    if len(records) != 134:
        raise SystemExit(f"expected 134 semantic records after excluding filing guides, found {len(records)}")
    grouped = {group_id: [] for group_id, _ in GROUPS}
    for record in records:
        grouped[group_for(record[0])].append(record)
    for group_records in grouped.values():
        group_records.sort(key=lambda record: label(record[0]).casefold())

    def concept_id(caption: str) -> str:
        return REUSE.get(caption, f"bokheim:protestant-denomination:{slug(caption)}")

    lines = [
        "-- Curate semantic denominations and movements beneath the LCC",
        "-- Other Protestant denominations leaf. Alphabetic filing-guide records are excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-protestant-denominations-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts = [(f"bokheim:protestant-denominations:{gid}", glabel) for gid, glabel in GROUPS]
    concepts += [(concept_id(caption), label(caption)) for caption, _, _ in records if caption not in REUSE]
    lines += [f"    ({sql(cid)}, {sql(clabel)}){',' if i + 1 < len(concepts) else ';'}" for i, (cid, clabel) in enumerate(concepts)]

    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES"]
    edges = []
    for gi, (gid, _) in enumerate(GROUPS, 1):
        group_id = f"bokheim:protestant-denominations:{gid}"
        edges.append((group_id, PARENT_ID, 0, gi * 10))
        for ci, (caption, _, _) in enumerate(grouped[gid], 1):
            edges.append((concept_id(caption), group_id, 1 if caption in REUSE else 0, ci * 10))
    lines += [f"    ({sql(cid)}, {sql(pid)}, {ordinal}, {order}){',' if i + 1 < len(edges) else ';'}" for i, (cid, pid, ordinal, order) in enumerate(edges)]

    lines += ["", "-- Priority 4 is more specific than the existing denomination ranges at priority 3.",
              "INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES"]
    selectors = []
    for caption, start, end in records:
        cid = concept_id(caption)
        ordinal = REUSE_SELECTOR_ORDINAL[caption] if caption in REUSE else 0
        selectors.append((cid, f"{start}..{end}@4", ordinal))
    lines += [f"    ({sql(cid)}, 'lcc', {sql(selector)}, {ordinal}){',' if i + 1 < len(selectors) else ';'}" for i, (cid, selector, ordinal) in enumerate(selectors)]

    lines += ["", "INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES"]
    source_labels = []
    for caption, _, _ in records:
        cid = concept_id(caption)
        ordinal = REUSE_LABEL_ORDINAL[caption] if caption in REUSE else 0
        source_labels.append((cid, caption, ordinal))
    lines += [f"    ({sql(cid)}, {sql(caption)}, {ordinal}){',' if i + 1 < len(source_labels) else ';'}" for i, (cid, caption, ordinal) in enumerate(source_labels)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("client/library-model/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("client/library-model/data/migrations/20260817_protestant_denominations_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")


if __name__ == "__main__":
    main()
