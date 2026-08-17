#!/usr/bin/env python3
"""Generate the curated, indirect Subject bibliography LCC expansion."""

from __future__ import annotations

import argparse
import re
import sqlite3
import unicodedata
from pathlib import Path


SUBJECT_BIBLIOGRAPHY_ID = "unified:4eb4347a-05ba-59a2-b1ce-47d125734731"

GROUPS: list[tuple[str, str, tuple[str, ...]]] = [
    ("reference-scholarship", "Reference, Libraries & Scholarship", (
        "Academies. Societies. Universities. Museums", "Almanacs", "Ana",
        "Archives. Records", "Authorship (General)", "Biography. Genealogy. Heraldry",
        "Calendars", "Collectors and collecting", "Directories", "Encyclopedias",
        "Exhibitions", "Indexes. Abstracts", "Manuscripts", "Names", "Nonbook materials",
        "Pamphlets", "Periodicals, newspapers, and other serials",
    )),
    ("humanities-culture", "Humanities & Culture", (
        "Aesthetics", "Anthropology and ethnology", "Archaeology", "Civilization",
        "Costume", "Curiosities and wonders", "Days. Holidays. Festivals. Birthdays",
        "Death", "Domestic economy. Home economics", "Ethics", "Etiquette", "Folklore",
        "Funeral customs. Disposal of the bodies of the dead", "Geography and travels. Maps. Cartography",
        "History", "Humanities", "Letters", "Literature", "Philology and linguistics",
        "Philosophy", "Proverbs", "Seals. Sigillography", "Symbolism",
    )),
    ("society-law-education", "Society, Law & Education", (
        "Communication. Mass media", "Criminology", "Disasters", "Education", "Forecasting",
        "Human engineering", "International law and relations", "Law",
        "Political and social sciences", "Statistics", "Temperance", "Women",
    )),
    ("religion-belief", "Religion & Belief", (
        "Devil", "Flagellants and flagellation", "Freemasons", "Jews", "Occultism",
        "Theology and religion",
    )),
    ("natural-sciences-mathematics", "Natural Sciences & Mathematics", (
        "Astronomy", "Atmospheric radioactivity", "Bacteriology. Microbiology", "Biology",
        "Birds. Ornithology", "Botany", "Chemistry", "Cosmic physics", "Electricity",
        "Entomology", "Environment (General and human). Human ecology", "Geodesy",
        "Geology. Mineralogy. Paleontology", "Geophysics", "Mathematics", "Meteorology",
        "Microscopy", "Nature", "Parasitology", "Physics", "Science. Natural history",
        "Zoology",
    )),
    ("medicine-health-disability", "Medicine, Health & Disability", (
        "Beauty culture", "Blind, The", "Deaf-mutes", "Handicapped. People with disabilities",
        "Medicine", "Mental retardation. People with mental disabilities",
        "People with physical disabilities", "Psychology", "Rehabilitation", "Stammering",
        "Suicide", "Toxicology", "Vocational rehabilitation",
    )),
    ("technology-engineering", "Technology & Engineering", (
        "Aerospace technology", "Atomic energy and power. Nuclear engineering", "Automation",
        "Computer science. Electronic data processing", "Electronics", "Engineering", "Explosives",
        "Radar", "Radio", "System analysis. Systems engineering. Operations research", "Television",
        "Useful arts and applied science. Technology", "Water supply. Ground water",
    )),
    ("agriculture-food-natural-resources", "Agriculture, Food & Natural Resources", (
        "Agriculture", "Bees and bee culture", "Cats", "Coffee", "Cotton", "Dairying",
        "Fish culture. Aquaculture", "Fishery technology. Fishery processing",
        "Fishing and fisheries. Angling", "Food service. Restaurants", "Forestry",
        "Fur. Fur-bearing animals", "Gardening. Horticulture. Floriculture. Landscape gardening and architecture",
        "Horse, The. Horsemanship", "Ice cream, ices, etc.", "Parks", "Peat", "Pets", "Salt",
        "Sugar", "Tobacco. Smoking", "Wine and wine making", "Wool",
    )),
    ("arts-media-recreation", "Arts, Media & Recreation", (
        "Cards and card playing", "Chess", "Drama. Theater", "Emblems",
        "Erotic literature, facetiae, curiosa, etc.", "Fables", "Fencing and dueling", "Fiction",
        "Fine arts (Visual arts). The arts (General)", "Flags", "Furniture", "Gems",
        "Gold and silversmiths' work", "Graffiti", "Graphology", "Gymnastics. Physical education",
        "Handicraft", "Performing arts", "Photography", "Poetry", "Pottery. Ceramics", "Riddles",
        "Sports. Amusements. Recreation", "Swimming", "Time and timekeepers", "Toys",
    )),
    ("transport-infrastructure", "Transport & Infrastructure", (
        "Automobiles. Automobile travel. Motor vehicles", "Canals", "Hospitality industry. Hotels",
        "Military science", "Naval science", "Navigation", "Railroads", "Roads. Highways",
    )),
    ("industry-materials", "Industry & Materials", (
        "Bells", "Glass. Glassware", "Iron and steel", "Metals. Metallurgy", "Mines and mining",
        "Numismatics", "Petroleum", "Rubber (\"India-rubber,\" synthetic rubber)",
    )),
]

LABEL_OVERRIDES = {
    "Academies. Societies. Universities. Museums": "Academies, Societies, Universities & Museums",
    "Ana": "Ana & Literary Miscellanies",
    "Blind, The": "Blindness & Blind People",
    "Deaf-mutes": "Deafness & Deaf People",
    "Handicapped. People with disabilities": "Disability & Disabled People",
    "Mental retardation. People with mental disabilities": "Intellectual Disabilities",
    "People with physical disabilities": "Physical Disabilities",
    "Stammering": "Stuttering",
    "Environment (General and human). Human ecology": "Environment & Human Ecology",
    "Fine arts (Visual arts). The arts (General)": "Fine Arts & Visual Arts",
    "Useful arts and applied science. Technology": "Technology & Applied Science",
}


def slug(value: str) -> str:
    ascii_value = unicodedata.normalize("NFKD", value).encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z0-9]+", "-", ascii_value.lower()).strip("-")


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def display_label(caption: str) -> str:
    if caption in LABEL_OVERRIDES:
        return LABEL_OVERRIDES[caption]
    replacements = {
        ". ": ", ", " and ": " & ", " (General and human)": "",
        " (General)": "", "The arts (General)": "Arts", "Useful arts and applied science": "Applied Science",
    }
    label = caption
    for old, new in replacements.items():
        label = label.replace(old, new)
    label = label.rstrip(".").title()
    for word in ("And", "As", "Of", "The", "To", "With"):
        label = re.sub(rf"(?<= )({word})(?= )", word.lower(), label)
    return label.replace("Etc.", "etc.")


def load_records(database: Path) -> dict[str, tuple[str, str]]:
    query = """
        SELECT cr.caption, span.start_number, coalesce(span.end_number, span.start_number)
        FROM classification_record cr
        JOIN caption_hierarchy h0 ON h0.record_id = cr.record_id
          AND h0.ordinal = 0 AND h0.kind = 'caption' AND h0.label = 'Subject bibliography'
        LEFT JOIN caption_hierarchy h1 ON h1.record_id = cr.record_id AND h1.ordinal = 1
        JOIN classification_span span ON span.record_id = cr.record_id AND span.ordinal = 0
        WHERE h1.record_id IS NULL AND span.start_number GLOB 'Z*'
        ORDER BY CAST(substr(span.start_number, 2) AS REAL), span.start_number
    """
    with sqlite3.connect(database) as connection:
        rows = connection.execute(query).fetchall()
    return {caption: (start, end) for caption, start, end in rows}


def render(records: dict[str, tuple[str, str]]) -> str:
    assignments = {caption: group_id for group_id, _, captions in GROUPS for caption in captions}
    if len(assignments) != sum(len(captions) for _, _, captions in GROUPS):
        raise SystemExit("a subject bibliography caption is assigned more than once")
    missing = sorted(set(records) - set(assignments))
    stale = sorted(set(assignments) - set(records))
    if missing or stale:
        raise SystemExit(f"group assignments do not match snapshot; missing={missing!r}, stale={stale!r}")

    lines = [
        "-- Add the immediate LCC Subject bibliography detail without flattening all",
        "-- topics directly beneath Subject bibliography or reusing ordinary subject concepts.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-subject-bibliography-migration.py.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
        "INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES",
    ]
    concepts: list[tuple[str, str]] = []
    for group_id, group_label, captions in GROUPS:
        concepts.append((f"bokheim:subject-bibliography:{group_id}", group_label))
        concepts.extend((f"bokheim:subject-bibliography:{slug(caption)}", display_label(caption)) for caption in captions)
    lines.extend(
        f"    ({sql(concept_id)}, {sql(label)}){',' if index + 1 < len(concepts) else ';'}"
        for index, (concept_id, label) in enumerate(concepts)
    )

    lines += ["", "INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES"]
    parents: list[tuple[str, str, int, int]] = []
    for group_order, (group_id, _, captions) in enumerate(GROUPS, 1):
        group_concept = f"bokheim:subject-bibliography:{group_id}"
        parents.append((group_concept, SUBJECT_BIBLIOGRAPHY_ID, 0, group_order * 10))
        for topic_order, caption in enumerate(captions, 1):
            parents.append((f"bokheim:subject-bibliography:{slug(caption)}", group_concept, 0, topic_order * 10))
    lines.extend(
        f"    ({sql(child)}, {sql(parent)}, {ordinal}, {order}){',' if index + 1 < len(parents) else ';'}"
        for index, (child, parent, ordinal, order) in enumerate(parents)
    )

    lines += ["", "-- Priority 2 makes each topic more specific than the existing Z5051..Z7999@1 parent.",
              "INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES"]
    selectors: list[tuple[str, str]] = []
    for _, _, captions in GROUPS:
        for caption in captions:
            start, end = records[caption]
            # The retrospective record for Wool says Z7971..Z7965. Do not emit
            # an unusable inverted selector or guess at a corrected endpoint.
            if caption == "Wool" and end == "Z7965":
                end = start
            selectors.append((f"bokheim:subject-bibliography:{slug(caption)}", f"{start}..{end}@2"))
    lines.extend(
        f"    ({sql(concept_id)}, 'lcc', {sql(selector)}, 0){',' if index + 1 < len(selectors) else ';'}"
        for index, (concept_id, selector) in enumerate(selectors)
    )

    lines += ["", "-- Preserve the exact historical source captions separately from modern visible labels.",
              "INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES"]
    source_labels = [(f"bokheim:subject-bibliography:{slug(caption)}", caption) for _, _, captions in GROUPS for caption in captions]
    lines.extend(
        f"    ({sql(concept_id)}, {sql(caption)}, 0){',' if index + 1 < len(source_labels) else ';'}"
        for index, (concept_id, caption) in enumerate(source_labels)
    )
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, default=Path("client/library-model/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--output", type=Path, default=Path("client/library-model/data/migrations/20260817_subject_bibliography_lcc_expansion.sql"))
    args = parser.parse_args()
    args.output.write_text(render(load_records(args.snapshot)), encoding="utf-8")


if __name__ == "__main__":
    main()
