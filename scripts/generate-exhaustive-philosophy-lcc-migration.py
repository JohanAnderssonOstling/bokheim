#!/usr/bin/env python3
"""Generate the exhaustive semantic B-BJ Philosophy expansion."""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
import uuid
from collections import Counter, defaultdict
from pathlib import Path


HERE = Path(__file__).resolve().parent
HISTORY_PATH = HERE / "generate-exhaustive-history-lcc-migration.py"
SPEC = importlib.util.spec_from_file_location("philosophy_exhaust_core", HISTORY_PATH)
assert SPEC and SPEC.loader
history = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = history
SPEC.loader.exec_module(history)
core = history.core

NAMESPACE = uuid.UUID("da571da4-8b17-5b22-bce8-3403e6d06eca")
PHILOSOPHY = "unified:82e05c69-80eb-57ac-b130-86c0f9a53371"
PHILOSOPHY_GENERAL = "bokheim:subject:philosophy:general-reference"
PHILOSOPHY_PERIODS = "bokheim:subject:philosophy:history:periods"
PHILOSOPHY_REGIONS = "bokheim:subject:philosophy:history:regions"
LOGIC = "unified:247f06d2-854f-5eae-9474-55d966152f6d"
METAPHYSICS = "unified:13894dab-8bff-506c-8527-8c72fe0e1454"
PSYCHOLOGY = "unified:91e9ffa6-f917-5bc8-83c8-c0c5933abcd3"
PARAPSYCHOLOGY = "unified:fd3ae202-02c8-5b3c-ab29-5369cf76a89a"
OCCULT = "unified:2548d0b0-c661-55b6-9b71-5ca18bf087f2"
ETHICS = "unified:d18bb60c-150e-57ff-a1c0-6b3aa42a58e3"
ETIQUETTE = "unified:fd6f06c6-ea57-59e2-be19-42cf640db089"
ART_THEORY = "bokheim:subject:arts:general-theory-institutions"
AESTHETICS = "bokheim:subject:philosophy:aesthetics"

ROOT_ANCHORS = {
    "aesthetics": AESTHETICS,
    "ethics": ETHICS,
    "logic": LOGIC,
    "occult sciences": OCCULT,
    "parapsychology": PARAPSYCHOLOGY,
    "philosophy general": PHILOSOPHY_GENERAL,
    "psychology": PSYCHOLOGY,
    "social usages etiquette": ETIQUETTE,
    "speculative philosophy": METAPHYSICS,
}
ELIGIBLE_ROOTS = frozenset(ROOT_ANCHORS)

FORM_TERMS = (
    "abstracts", "addresses essays lectures", "almanacs", "bibliograph",
    "biography", "collected works", "collections nonserial", "concordances",
    "congresses", "curiosa miscellanea", "dictionaries", "directories",
    "early works through", "facetiae satire",
    "elementary textbooks", "encyclopedias", "essays lectures", "examinations",
    "general works", "handbooks", "indexes", "juvenile literature",
    "juvenile works", "manuals", "modern works", "original editions",
    "original text", "outlines syllabi", "partial editions",
    "periodicals", "problems exercises", "selections by date", "separate works",
    "forms blanks", "satire etc", "serials", "societies", "study and teaching",
    "symbolic works", "textbooks",
    "translations", "works in the form of stories", "yearbooks",
)
PERSON_OR_WORK_TERMS = (
    "ethical philosophers", "individual authors", "individual logicians",
    "individual philosophers", "individual philoosphers", "individual psychologists",
    "individual spirit authors", "individual thinkers", "individual writers",
    "personal names", "separate works", "other works a z", "works a z",
    "works by title", "biography autobiography",
)
INSTITUTION_TERMS = (
    "individual institutions", "individual schools", "individual societies",
    "universities and colleges", "research institutes",
)
TRANSPARENT = {
    "by country", "by period", "by region or country", "general", "history general",
    "miscellaneous", "other", "special", "special topics",
}
DATED_PERSON_RE = re.compile(
    r"^[^,;:]{2,100},\s*(?:ca\.?\s*)?(?:fl\.?\s*)?\d{2,4}(?:\s*[-–/]\s*\d{0,4})?",
    re.I,
)


def eligible(record: core.Record) -> bool:
    if not record.path:
        return False
    root = core.normalize_label(record.path[0])
    if root not in ELIGIBLE_ROOTS:
        return False
    start = record.start
    return bool(
        re.match(r"^B\d", start)
        or start.startswith(("BC", "BD", "BF", "BH", "BJ"))
    )


def is_form(label: str) -> bool:
    normalized = core.normalize_label(label)
    return core.is_form_caption(label) or any(term in normalized for term in FORM_TERMS)


def exclusion(record: core.Record) -> str | None:
    for component in record.full_path[1:]:
        normalized = core.normalize_label(component)
        if core.FILING_RE.search(component) or core.ALPHA_BUCKET_RE.fullmatch(component.strip()):
            return "alphabetic subject, author, or title filing tree"
        if (
            any(term in normalized for term in PERSON_OR_WORK_TERMS)
            or DATED_PERSON_RE.match(component.strip())
            or core.PERSON_NAME_RE.match(component.strip())
        ):
            return "individual philosopher, psychologist, or named-work tree"
        if any(term in normalized for term in INSTITUTION_TERMS):
            return "institution or organization authority tree"
        if is_form(component):
            return "bibliographic or document-form tree"
    return None


def is_transparent(label: str) -> bool:
    normalized = core.normalize_label(label)
    return (
        normalized in TRANSPARENT
        or normalized.startswith("other ")
        or normalized.startswith("special topics")
        or normalized.startswith("by region or country")
    )


def philosophy_root_anchor(record: core.Record) -> str:
    root = core.normalize_label(record.path[0])
    if root != "philosophy general":
        return ROOT_ANCHORS[root]
    route = [core.normalize_label(component) for component in record.full_path]
    if any(component.startswith("by region or country") for component in route):
        return PHILOSOPHY_REGIONS
    if any(component in {
        "ancient 600 b c 430 a d", "medieval 430 1450", "renaissance",
        "modern 1450 1600", "modern 450 1600",
    } for component in route):
        return PHILOSOPHY_PERIODS
    return PHILOSOPHY_GENERAL


def build_manifest(taxonomy_path: Path, snapshot_path: Path) -> dict[str, object]:
    taxonomy = core.load_taxonomy(taxonomy_path)
    records, _children, _labels, _spans, _classes, _sorted = core.load_snapshot(snapshot_path)
    concepts = taxonomy["concepts"]
    routes = taxonomy["routes"]
    selectors = taxonomy["selectors"]
    source_labels = taxonomy["labels"]
    parents = taxonomy["parents"]

    scope_roots = {PHILOSOPHY, PSYCHOLOGY, "unified:3b1b59d5-13d8-5505-a8b1-d1c6ef05ce89", ART_THEORY}
    scope_ids = {
        cid for cid, values in routes.items()
        if any(any(root in {ancestor for ancestor, *_ in parents.get(cid, [])} for root in scope_roots) for _ in [0])
    }
    # Route-prefix membership is needed for descendants deeper than one edge.
    scope_labels = {concepts[root] for root in scope_roots}
    scope_ids.update(
        cid for cid, values in routes.items()
        if any(any(label in scope_labels for label in route[:4]) for route in values)
    )
    scope_ids.update(scope_roots)

    span_concepts: dict[tuple[str, str], list[tuple[str, set[str]]]] = defaultdict(list)
    for cid in scope_ids:
        names = {core.normalize_label(concepts[cid])}
        names.update(core.normalize_label(label) for label, _ in source_labels.get(cid, []))
        for system, selector, _ in selectors.get(cid, []):
            if system == "lcc" and (span := core.selector_span(selector)):
                span_concepts[span].append((cid, names))

    represented: dict[int, str] = {}
    for record in records.values():
        if not eligible(record):
            continue
        normalized = core.normalize_label(record.caption)
        matches = []
        for cid, names in span_concepts.get((record.start, record.end), []):
            if normalized in names or any(
                len(name) > 10 and (normalized.startswith(name) or name.startswith(normalized))
                for name in names
            ):
                matches.append(cid)
        if matches:
            # Prefer the most specific existing route.
            represented[record.record_id] = max(matches, key=lambda cid: max(map(len, routes[cid])))

    eligible_records = [record for record in records.values() if eligible(record)]
    by_full_path: dict[tuple[str, ...], list[core.Record]] = defaultdict(list)
    for record in eligible_records:
        by_full_path[record.full_path].append(record)

    assigned = dict(represented)
    new_nodes: dict[str, str] = {AESTHETICS: "Aesthetics"} if AESTHETICS not in concepts else {}
    new_edges: list[tuple[str, str]] = []
    if AESTHETICS not in concepts:
        new_edges.extend(((AESTHETICS, PHILOSOPHY), (AESTHETICS, ART_THEORY)))
    desired_selectors: dict[str, set[str]] = defaultdict(set)
    desired_labels: dict[str, set[str]] = defaultdict(set)
    desired_selectors[AESTHETICS].add("BH1..BH301@2")
    desired_labels[AESTHETICS].add("Aesthetics")

    virtual_children: dict[tuple[str, str], list[str]] = defaultdict(list)
    for child, values in parents.items():
        for parent, _, _ in values:
            virtual_children[(parent, core.normalize_label(concepts[child]))].append(child)
    for child, parent in new_edges:
        virtual_children[(parent, core.normalize_label(new_nodes[child]))].append(child)

    max_priority = {
        cid: max((core.selector_priority(value) for system, value, _ in values if system == "lcc"), default=4)
        for cid, values in selectors.items()
    }
    max_priority[AESTHETICS] = 2
    excluded = Counter()
    examples: dict[str, list[str]] = defaultdict(list)
    transparent_count = 0
    collapsed_count = 0
    reused_count = 0

    def add_excluded(reason: str, record: core.Record) -> None:
        excluded[reason] += 1
        if len(examples[reason]) < 20:
            examples[reason].append(f"{record.start}..{record.end}: " + " / ".join(record.full_path))

    def nearest(record: core.Record) -> tuple[core.Record | None, str]:
        for size in range(len(record.path), 0, -1):
            candidates = [
                ancestor for ancestor in by_full_path.get(record.path[:size], [])
                if ancestor.record_id in assigned and core.span_is_within(record.start, record.end, ancestor)
            ]
            if candidates:
                ancestor = max(candidates, key=lambda item: (len(item.path), item.record_id))
                return ancestor, assigned[ancestor.record_id]
        return None, philosophy_root_anchor(record)

    eligible_records.sort(key=lambda record: (
        len(record.path), core.number_key(record.start) or (10**9, 0.0, ()), record.record_id
    ))
    for record in eligible_records:
        if record.record_id in assigned:
            continue
        if reason := exclusion(record):
            add_excluded(reason, record)
            continue
        ancestor, parent_id = nearest(record)
        if is_transparent(record.caption):
            assigned[record.record_id] = parent_id
            transparent_count += 1
            continue
        if ancestor is not None and record.start == ancestor.start and record.end == ancestor.end:
            assigned[record.record_id] = parent_id
            desired_labels[parent_id].add(record.caption)
            collapsed_count += 1
            continue

        key = (parent_id, core.normalize_label(record.caption))
        existing = virtual_children.get(key, [])
        if existing:
            target = existing[0]
            reused_count += 1
        else:
            target = "bokheim:philosophy-lcc-full:" + str(uuid.uuid5(
                NAMESPACE, f"{parent_id}\n{core.normalize_label(record.caption)}"
            ))
            if target not in concepts and target not in new_nodes:
                new_nodes[target] = record.caption
            new_edges.append((target, parent_id))
            virtual_children[key].append(target)

        priority = min(9, max_priority.get(parent_id, 4) + 1)
        desired_selectors[target].add(f"{record.start}..{record.end}@{priority}")
        desired_labels[target].add(record.caption)
        max_priority[target] = max(max_priority.get(target, 0), priority)
        assigned[record.record_id] = target

    new_edges.sort(key=lambda edge: (
        edge[1], min(desired_selectors.get(edge[0], {"ZZZ"})).split("@")[0],
        new_nodes.get(edge[0], concepts.get(edge[0], edge[0])),
    ))
    return {
        "version": 1,
        "source": "Library of Congress MDSConnect Classification 2016 retrospective",
        "eligible_snapshot_records": len(eligible_records),
        "represented_snapshot_records": len(represented),
        "assigned_snapshot_records": len(assigned),
        "transparent_structural_records": transparent_count,
        "collapsed_same_span_records": collapsed_count,
        "reused_direct_children": reused_count,
        "new_nodes": [{"concept_id": cid, "preferred_label": label} for cid, label in sorted(new_nodes.items())],
        "edges": [{"child_id": child, "parent_id": parent} for child, parent in new_edges],
        "selectors": [
            {"concept_id": cid, "selector": value}
            for cid in sorted(desired_selectors) for value in sorted(desired_selectors[cid])
        ],
        "labels": [
            {"concept_id": cid, "label": value}
            for cid in sorted(desired_labels) for value in sorted(desired_labels[cid])
        ],
        "excluded": {
            reason: {"count": count, "examples": examples[reason]}
            for reason, count in sorted(excluded.items())
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--taxonomy", type=Path, default=Path("shared/subject-projection/data/unified-taxonomy-v2.sqlite3"))
    parser.add_argument("--snapshot", type=Path, default=Path("shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--manifest", type=Path, default=Path("shared/subject-projection/data/philosophy-lcc-exhaustive-manifest.json"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_philosophy_lcc_exhaustive.sql"))
    args = parser.parse_args()
    manifest = build_manifest(args.taxonomy, args.snapshot)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    sql = history.render_sql(manifest, args.taxonomy)
    sql = sql.replace(
        "Exhaustive semantic LCC continuation beneath the curated History hierarchy.",
        "Exhaustive semantic LCC continuation beneath the curated Philosophy, Psychology, and Aesthetics hierarchies.",
    ).replace(
        "Generated by scripts/generate-exhaustive-history-lcc-migration.py",
        "Generated by scripts/generate-exhaustive-philosophy-lcc-migration.py",
    )
    args.output.write_text(sql, encoding="utf-8")
    print(json.dumps({
        "eligible_snapshot_records": manifest["eligible_snapshot_records"],
        "represented_snapshot_records": manifest["represented_snapshot_records"],
        "assigned_snapshot_records": manifest["assigned_snapshot_records"],
        "new_nodes": len(manifest["new_nodes"]),
        "edges": len(manifest["edges"]),
        "selectors": len(manifest["selectors"]),
        "reused_direct_children": manifest["reused_direct_children"],
        "transparent_structural_records": manifest["transparent_structural_records"],
        "excluded": {key: value["count"] for key, value in manifest["excluded"].items()},
    }, indent=2))


if __name__ == "__main__":
    main()
