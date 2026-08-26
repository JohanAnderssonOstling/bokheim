#!/usr/bin/env python3
"""Generate the curated Civilization and Historical Eras LCC expansion."""

from __future__ import annotations

import argparse
import json
import sqlite3
import uuid
from collections import defaultdict
from pathlib import Path


NAMESPACE = uuid.UUID("acaf15eb-f63d-5fc4-a330-903884041d72")

CIV_PERIOD = "unified:4f373700-3ceb-5c13-bcda-7f977ff3676d"
MEDIEVAL = "bokheim:lcc-detail:a410cc0b-a04b-5941-b8a7-91d6d83f2b06"
MODERN = "bokheim:lcc-detail:987dfd72-7a3f-5b56-8285-f9c107c495e0"
RACE = "unified:d09ebc56-2e38-52e2-87a2-354274c8bae9"
SPECIAL_CIV = "bokheim:lcc-detail:83d1abbe-f7fe-5bc7-a3e3-480ea4d47b39"
RENAISSANCE = "unified:1a62f114-665e-51e7-ab42-5140dd2672fc"

CENTURY_18 = "unified:15f9e11b-46f6-5b73-ad4c-937b428b8e48"
CENTURY_19 = "unified:89429483-701a-58e4-be01-ddf01d05172a"
INTERWAR = "unified:9f7da15c-2250-59f8-9ea9-8e174130ce0b"
POSTWAR_1945 = "bokheim:lcc-detail:620181de-e920-5461-8dec-9822a61824b4"
POSTWAR_1965 = "bokheim:lcc-detail:baa859f3-fb35-5355-a3de-b630411836c7"
POSTWAR_1989 = "bokheim:lcc-detail:f0d8ac63-12d8-5b2f-bed6-8644c8aa15f2"
INTERWAR_SOCIAL = "bokheim:lcc-detail:bde19a4d-40c4-5b6e-9003-a30f6916b2af"
FASCISM = "bokheim:lcc-detail:637e5d44-3139-55c8-92fc-1bada6869c42"


def concept_id(parent: str, label: str) -> str:
    return "bokheim:lcc-curated:" + str(uuid.uuid5(NAMESPACE, f"{parent}\n{label}"))


def build() -> dict[str, object]:
    nodes: dict[str, str] = {}
    edges: set[tuple[str, str]] = set()
    selectors: dict[str, set[str]] = defaultdict(set)
    labels: dict[str, set[str]] = defaultdict(set)

    def add(parent: str, label: str, selector: str | None = None,
            source_label: str | None = None, reuse: str | None = None) -> str:
        cid = reuse or concept_id(parent, label)
        if reuse is None:
            nodes[cid] = label
        edges.add((cid, parent))
        if selector:
            selectors[cid].add(selector)
        if source_label or selector:
            labels[cid].add(source_label or label)
        return cid

    # Civilization chronology (CB351-CB430).
    for label, selector in (
        ("7th Century", "CB353.7..CB353.7@5"),
        ("8th Century", "CB353.8..CB353.8@5"),
        ("10th Century", "CB354..CB354@5"),
        ("11th Century", "CB354.3..CB354.3@5"),
        ("12th Century", "CB354.6..CB354.6@5"),
        ("13th Century", "CB355..CB355@5"),
    ):
        add(MEDIEVAL, label, selector)

    add(MODERN, "Renaissance & Reformation", "CB359..CB359@5", "Renaissance and Reformation")
    add(MODERN, "Renaissance", "CB361..CB361@5", reuse=RENAISSANCE)
    for label, selector, source in (
        ("13th–14th Centuries", "CB365..CB365@5", "13th-14th centuries"),
        ("1401–1550", "CB367..CB367@5", "1401-1550"),
        ("1551–1600", "CB369..CB369@5", "1551-1600"),
    ):
        add(RENAISSANCE, label, selector, source)
    for label, selector, source in (
        ("16th–17th Centuries", "CB401..CB401@5", "16th-17th centuries"),
        ("18th Century", "CB411..CB411@5", None),
        ("19th Century", "CB415..CB415@5", None),
        ("1900–1970", "CB425..CB425@5", "1900-1970"),
        ("1971–Present", "CB428..CB428@5", "1971-")
    ):
        add(MODERN, label, selector, source)

    # Retain obsolete LCC racial captions as provenance, never as assertions.
    race_context = add(
        RACE, "Race and Civilization in Historical Thought",
        "CB201..CB241.21@5", "Caucasian. Aryan"
    )
    for old in (
        "Nordic", "Alpine. Celtic", "Mediterranean. Latin", "Black",
        "Semitic", "Northern and Southern",
    ):
        labels[race_context].add(old)
    for label, selector, source in (
        ("Western Civilizations", "CB245..CB245@5", "Occidental. Western"),
        ("East–West Interactions", "CB251..CB251@5", "Eastern and Western"),
        ("Eastern Civilizations", "CB253..CB256@5", "Oriental. Eastern"),
        ("Buddhist Civilization", "CB255..CB255@5", "Buddhist"),
        ("Confucian Civilization", "CB256..CB256@5", "Confucian"),
        ("Developing Regions & Postcolonial Societies", "CB271..CB271@5",
         "Developing countries. Third world"),
    ):
        add(SPECIAL_CIV, label, selector, source)

    # Eighteenth-century European conflicts.
    austrian = add(CENTURY_18, "War of the Austrian Succession, 1740–1748",
                   "D291..D294@5", "War of Austrian Succession, 1740-1748")
    for label, selector, source in (
        ("Silesian Campaigns & International Relations", "D293..D293@6", "Silesian campaigns. International relations"),
        ("Rhine and Danube Campaigns, 1741–1744", "D293.2..D293.3@6", "Rhine and Danube, 1741-1744"),
        ("Italian Campaigns, 1741–1748", "D293.4..D293.55@6", "Italy, 1741-1748"),
        ("Netherlands Campaigns, 1743–1748", "D293.6..D293.7@6", "Netherlands, 1743-1748"),
        ("Colonial and Naval Operations, 1739–1748", "D293.8..D293.9@6", "Colonial, 1739-1748. Naval operations"),
        ("Peace of Aix-la-Chapelle, 1748", "D294..D294@6", None),
    ):
        add(austrian, label, selector, source)
    seven = add(CENTURY_18, "Seven Years’ War, 1756–1763", "D297..D297@5",
                "Seven Years' War, 1756-1763")
    labels[seven].add("General international aspects, results, etc. Peace of Paris, 1763")

    # Nineteenth-century Europe and the Eastern Question.
    add(CENTURY_19, "Historiography", "D352.9..D352.9@5")
    add(CENTURY_19, "Europe, 1801–1914", "D359..D359@5", "Europe in the 19th century")
    add(CENTURY_19, "Social Life & Customs", "D360..D360@5")
    add(CENTURY_19, "Military History", "D361..D361@5")
    add(CENTURY_19, "Naval History", "D362..D362@5")
    add(CENTURY_19, "Political & Diplomatic History", "D363..D363@5")
    eastern = add(CENTURY_19, "Eastern Question", "D371..D379@5")
    for label, selector, source in (
        ("Origins to 1800", "D373..D373@6", "Early to 1800"),
        ("19th Century", "D374..D375.3@6", None),
        ("Panslavism", "D377.3..D377.5@6", None),
        ("Central Asian Question", "D378..D378.5@6", None),
        ("Far Eastern Question", "D379..D379@6", "Far East"),
    ):
        add(eastern, label, selector, source)
    eastern_19 = concept_id(eastern, "19th Century")
    add(eastern_19, "Conference of Berlin, 1878", "D375.3..D375.3@7")

    add(CENTURY_19, "Congress of Vienna and Aftermath, 1815–1830",
        "D383..D383@5", "Congress of Vienna. 1815-1830")
    period_1830 = add(CENTURY_19, "1830–1848", "D385..D387@5", "1830-1848")
    add(period_1830, "Revolutions of 1848", "D387..D387@6", "1848")
    add(CENTURY_19, "1848–1859", "D388..D389@5", "1848-1859")
    add(CENTURY_19, "1860–1870", "D391..D392@5", "1860-1870")
    later = add(CENTURY_19, "Later 19th Century", "D394..D400@5")
    add(later, "Military History", "D396..D396@6")
    add(later, "Political & Diplomatic History", "D397..D397@6")

    # Interwar and postwar international order.
    add(INTERWAR_SOCIAL, "Fascism", "D726.5..D726.5@5", reuse=FASCISM)
    add(INTERWAR, "Rome–Berlin Axis", "D728..D728@5", "Rome-Berlin axis")

    political_1945 = add(POSTWAR_1945, "Political & Diplomatic History", "D843..D847.2@5")
    atlantic = add(political_1945, "North Atlantic Treaty", "D845..D845@6")
    add(atlantic, "NATO", "D845.2..D845.2@7")
    soviet_1945 = add(political_1945, "Soviet Bloc", "D847..D847.2@6")
    add(soviet_1945, "Warsaw Pact", "D847.2..D847.2@7")
    political_1965 = add(POSTWAR_1965, "Political & Diplomatic History", "D849..D850@5")
    add(political_1965, "Soviet Bloc", "D850..D850@6")
    add(POSTWAR_1989, "Social & Intellectual History", "D857..D857@5")
    add(POSTWAR_1989, "Military History", "D858..D858@5")
    add(POSTWAR_1989, "Political & Diplomatic History", "D860..D860@5")

    ordered_edges = sorted(
        edges,
        key=lambda edge: (
            edge[1],
            min(selectors.get(edge[0], {"ZZZ"})).split("@")[0],
            nodes.get(edge[0], edge[0]),
        ),
    )
    return {
        "version": 1,
        "source": "Library of Congress MDSConnect Classification 2016 retrospective",
        "new_nodes": [{"concept_id": cid, "preferred_label": label} for cid, label in sorted(nodes.items())],
        "edges": [{"child_id": child, "parent_id": parent} for child, parent in ordered_edges],
        "selectors": [{"concept_id": cid, "selector": value} for cid in sorted(selectors) for value in sorted(selectors[cid])],
        "labels": [{"concept_id": cid, "label": value} for cid in sorted(labels) for value in sorted(labels[cid])],
        "excluded": [
            "bibliographic and form subdivisions", "alphabetic filing records",
            "named-person and named-work records", "generic Comprehensive works and Other records",
            "military unit and formation schedules",
        ],
    }


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def render(manifest: dict[str, object], taxonomy: Path) -> str:
    db = sqlite3.connect(taxonomy)
    edges = set(db.execute("SELECT concept_id,parent_concept_id FROM concept_parent"))
    existing_selectors = set(db.execute("SELECT concept_id,selector FROM source_selector WHERE system_id='lcc'"))
    existing_labels = set(db.execute("SELECT concept_id,label FROM source_label"))
    next_parent = defaultdict(int, {
        cid: value + 1 for cid, value in db.execute("SELECT concept_id,max(ordinal) FROM concept_parent GROUP BY concept_id")
    })
    next_selector = defaultdict(int, {
        cid: value + 1 for cid, value in db.execute("SELECT concept_id,max(ordinal) FROM source_selector GROUP BY concept_id")
    })
    next_label = defaultdict(int, {
        cid: value + 1 for cid, value in db.execute("SELECT concept_id,max(ordinal) FROM source_label GROUP BY concept_id")
    })
    next_display = defaultdict(int, {
        cid: value + 10 for cid, value in db.execute("SELECT parent_concept_id,max(display_order) FROM concept_parent GROUP BY parent_concept_id")
    })
    db.close()

    lines = [
        "-- Curated LCC detail for Civilization and Historical Eras.",
        "-- Generated by scripts/generate-civilization-historical-eras-migration.py.",
        "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
    ]
    nodes = manifest["new_nodes"]
    lines.append("INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES")
    lines += [f"    ({sql(r['concept_id'])},{sql(r['preferred_label'])}){',' if i + 1 < len(nodes) else ';'}" for i, r in enumerate(nodes)]
    lines.append("")

    rows = []
    for row in manifest["edges"]:
        child, parent = row["child_id"], row["parent_id"]
        if (child, parent) in edges:
            continue
        rows.append((child, parent, next_parent[child], next_display[parent]))
        next_parent[child] += 1
        next_display[parent] += 10
    lines.append("INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES")
    lines += [f"    ({sql(c)},{sql(p)},{o},{d}){',' if i + 1 < len(rows) else ';'}" for i, (c, p, o, d) in enumerate(rows)]
    lines.append("")

    rows = []
    for row in manifest["selectors"]:
        pair = (row["concept_id"], row["selector"])
        if pair in existing_selectors:
            continue
        rows.append((*pair, next_selector[pair[0]]))
        next_selector[pair[0]] += 1
    lines.append("INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES")
    lines += [f"    ({sql(c)},'lcc',{sql(v)},{o}){',' if i + 1 < len(rows) else ';'}" for i, (c, v, o) in enumerate(rows)]
    lines.append("")

    rows = []
    for row in manifest["labels"]:
        pair = (row["concept_id"], row["label"])
        if pair in existing_labels:
            continue
        rows.append((*pair, next_label[pair[0]]))
        next_label[pair[0]] += 1
    lines.append("INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES")
    lines += [f"    ({sql(c)},{sql(v)},{o}){',' if i + 1 < len(rows) else ';'}" for i, (c, v, o) in enumerate(rows)]
    lines += ["", "COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--taxonomy", type=Path, default=Path("shared/subject-projection/data/unified-taxonomy-v2.sqlite3"))
    parser.add_argument("--manifest", type=Path, default=Path("shared/subject-projection/data/civilization-historical-eras-manifest.json"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_civilization_historical_eras.sql"))
    args = parser.parse_args()
    manifest = build()
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    args.output.write_text(render(manifest, args.taxonomy), encoding="utf-8")
    print(json.dumps({key: len(manifest[key]) for key in ("new_nodes", "edges", "selectors", "labels")}, indent=2))


if __name__ == "__main__":
    main()
