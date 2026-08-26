#!/usr/bin/env python3
"""Exhaust semantic LCC detail below the curated History hierarchy.

The MDSConnect snapshot is a classification structure, not a ready-made user
taxonomy.  This generator follows represented C-F records recursively while
leaving catalog forms, name/place authorities, and military-unit schedules in
the source snapshot only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sqlite3
import sys
import uuid
from collections import Counter, defaultdict
from pathlib import Path


HERE = Path(__file__).resolve().parent
CORE_PATH = HERE / "generate-remaining-lcc-expansions.py"
SPEC = importlib.util.spec_from_file_location("history_exhaust_core", CORE_PATH)
assert SPEC and SPEC.loader
core = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = core
SPEC.loader.exec_module(core)

NAMESPACE = uuid.UUID("33218d22-95e0-5b5a-b267-2b65257270f8")
HISTORY_PREFIX = ("humanities", "history")
HISTORY_CLASSES = frozenset("CDEF")

ALLOWED_ROOTS = {
    "america", "archaeology", "auxiliary sciences of history general",
    "british america", "french america", "history general",
    "inscriptions epigraphy", "latin america spanish america", "numismatics",
    "technical chronology calendar", "united states",
}

# Some MDSConnect schedules express their title only as the first caption path
# element, not as a numbered classification record.  Anchor those structural
# roots to the equivalent concepts already curated in the unified hierarchy.
ROOT_ANCHORS = {
    "america": "bokheim:subject:history:americas",
    "archaeology": "unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554",
    "auxiliary sciences of history general": "unified:3cfd0be4-b1fe-599a-99d1-d79c4a585ea5",
    "british america": "bokheim:subject:history:americas:north-america",
    "french america": "bokheim:subject:history:americas",
    "history general": "unified:fdbd93fb-be52-58a0-80b5-278a79893d3e",
    "history of africa": "bokheim:subject:history:africa",
    "history of asia": "bokheim:subject:history:asia",
    "history of austria": "unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572",
    "history of balkan peninsula": "unified:d6daa986-0fa4-5394-945e-614d7b09d3ca",
    "history of belgium": "unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea",
    "history of central europe": "unified:68284cc9-f9d0-5474-a92c-caabd008b66e",
    "history of civilization": "unified:0b4e6056-a6bc-53ea-8233-fb2bd125726f",
    "history of czechoslovakia": "unified:3e65feaf-765f-54bb-88bb-bc3ae2b8452d",
    "history of eastern europe general": "unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec",
    "history of france": "unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143",
    "history of germany": "unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc",
    "history of great britain": "unified:f3fa3554-e35d-512f-9115-0052e24458fc",
    "history of greece": "unified:1773efe1-e08c-54df-a272-ac76cf96d508",
    "history of hungary": "unified:f3ca3e31-4ea8-5bfb-aaa5-0e92c65ac0de",
    "history of italy": "unified:97e56aeb-5265-540c-9edc-989bb0bcc66e",
    "history of liechtenstein": "unified:d8561f8d-aea2-5722-8410-60fe274b9a34",
    "history of low countries benelux countries": "unified:f1ffd3a1-ae5d-5720-9659-545b78d8ae0e",
    "history of luxembourg": "unified:a9b40fd5-19b3-5adc-8354-44d08510a2f0",
    "history of malta": "unified:e280b450-322d-528d-afe3-f905673ed43b",
    "history of netherlands holland": "unified:f9d54924-adb7-5cc4-8896-f87ed984e468",
    "history of northern europe scandinavia": "unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad",
    "history of oceania south seas": "bokheim:subject:history:oceania",
    "history of poland": "unified:b39c446a-3154-521c-8bf4-fa9c69c84ea5",
    "history of portugal": "unified:bcf05a2d-7986-5262-9c58-af0dcea68f64",
    "history of romanies history of gypsies": "unified:5d04d671-076c-5717-95b6-7ee46c747d2c",
    "history of russia soviet union former soviet republics": "unified:7dfc2889-a134-5a00-9ee2-e019655fd2bb",
    "history of spain": "unified:46059f35-84e2-546a-9b1d-b149357d5add",
    "history of switzerland": "unified:ea4b918f-9a52-516f-9bff-e2d73503516e",
    "history of the greco roman world": "unified:7677073c-c2fc-5085-8098-b4efd833ac1b",
    "inscriptions epigraphy": "unified:53d3e525-c60d-57c5-8e8c-9a747a09efa9",
    "latin america spanish america": "unified:1934d328-cf65-57fa-95b1-1c7e93cf34c3",
    "numismatics": "unified:c3f80b94-b0bc-591f-be31-ad9147c388de",
    "technical chronology calendar": "unified:d418a899-702f-5e9c-8236-3891a3e62aa1",
    "united states": "unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a",
    "united states local history": "unified:62708214-9d20-5e1f-b7ef-d3e4d591bab9",
}

# F-class places whose schedule records sit directly beneath the unnumbered
# United States local-history root even though the unified taxonomy already
# places them in the West. Reuse those concepts instead of creating a second
# state tree at the U.S. root.
US_LOCALITY_DIRECT_ANCHORS = {
    "alaska": "unified:27de458b-5965-5eed-8605-979a51bfd9fc",
    "utah": "unified:c88f2aa1-6718-5632-8f23-7b488f7c9e16",
}

# These branches enumerate documents, names, organizations, places, or units;
# none of their descendants become visible subjects.
EXCLUDED_BRANCH_TERMS = (
    "individual authors", "individual biography", "individual families",
    "individual persons", "individual rulers", "individual sovereigns",
    "individual institutions", "individual archives", "individual libraries",
    "individual museums", "individual monuments", "individual organizations",
    "individual societies", "individual formations", "individual regiments",
    "individual cities", "individual islands cities", "individual localities",
    "personal narratives", "memoirs by", "writings of", "letters of",
    "collections of writings", "contemporary accounts", "collected documents",
    "congressional documents", "parliamentary documents", "official records",
    "official reports", "party documents", "serials party documents",
    "regimental histories", "states and their participation",
    "branches of the party", "territorial divisions of the party",
    "affiliated organizations", "arms of the service", "arms of service",
    "special divisions regiments", "military units", "army units",
    "personal special", "writings", "collections nonserial collected works",
    "collections collected works", "museums collections", "sales catalogs",
    "relations with individual countries", "individual countries",
    "regions states territories", "regions provinces", "sections districts",
    "individual fighter squadrons", "individual torpedo squadrons",
    "individual patrol squadrons", "individual attack squadrons",
    "individual air groups", "individual wings", "individual squadrons",
    "individual camps", "individual islands", "american legion", "loyalist regiments",
    "bonaparte family", "father and mother of", "societies of veterans",
)

LOCALITY_BRANCH_TERMS = (
    "individual cities", "individual islands cities", "individual localities",
    "individual islands", "regions states territories", "regions provinces",
    "sections districts",
)

LOCALITY_ROUTE_TERMS = (
    "local history and description", "localities", "history and description",
    "history antiquities description", "cities towns", "cities and towns",
    "individual cities", "individual localities", "provinces regions",
    "regions provinces", "regions states territories", "sections districts",
    "united states local history",
)

FORM_TERMS = (
    "abstracts", "addresses", "almanacs", "annuals", "anecdotes",
    "bibliograph", "catalog", "collected works", "collections", "compends",
    "congresses", "contemporary newspaper", "dictionaries", "directories",
    "documentary collections", "encyclopedias", "essays lectures",
    "examinations", "fiction", "films slides", "gazetteers", "guidebooks",
    "handbooks", "indexes", "journal letters", "juvenile works", "manuals",
    "memoirs", "museums", "newspaper literature", "outlines syllabi",
    "pamphlets", "periodicals", "personal narratives", "pictorial works", "posters",
    "poems songs", "poetry ballads", "popular works", "problems exercises",
    "questions and answers",
    "registers lists", "sales catalogs", "serials", "slide collections",
    "sources and documents", "study and teaching", "tables outlines",
    "textbooks", "war poetry", "yearbooks", "juvenile literature",
    "comic and satiric works", "nonofficial publications", "official publications",
    "electronic information resources", "special documents", "chronological tables",
    "exhibitions", "serial", "societies",
)

MILITARY_UNIT_TERMS = (
    "army air forces", "army corps", "army units", "artillery regiments",
    "branches of the party", "brigades", "cavalry regiments", "corps darmee",
    "gas regiments", "individual formations", "infantry regiments",
    "machine gun regiments", "regimental histories", "regular army",
    "special units", "troop units", "volunteer troops irregulars", "air groups",
    "air wings", "squadrons", "the american army", "the british army",
    "the union army", "the confederate states army", "officers training corps",
    "quartermaster corps", "secret service signal corps", "british army",
)

# Structural records which can be traversed but should not add a visible node.
TRANSPARENT_LABELS = {
    "collective", "comprehensive works", "general", "general special",
    "general works", "individual", "individual battles", "individual campaigns",
    "individual events", "individual wars", "other", "other special",
    "special", "special events", "special questions and events", "history",
    "comprehensive description and history",
}

DATED_PERSON_RE = re.compile(r"^([^,;:]{2,80}),\s*(?:ca\.?\s*)?\d{1,4}(?:\s*[-–/]\s*\d{0,4})?", re.I)
PERSON_CONTEXT_RE = re.compile(
    r"(?:^|\b)(?:biography of|individual persons?|individual rulers?|"
    r"individual sovereigns?|personal names?)(?:\b|$)", re.I
)
UNIT_NUMBER_RE = re.compile(
    r"^(?:\d+(?:st|nd|rd|th)?\s+)?(?:army|battalion|brigade|cavalry|corps|"
    r"division|infantry|regiment|squadron|uhlans?)\b", re.I
)


def is_history_route(route: tuple[str, ...]) -> bool:
    return tuple(core.normalize_label(label) for label in route)[:2] == HISTORY_PREFIX


def allowed_root(record: core.Record, include_localities: bool = False) -> bool:
    if not record.path or record.start[:1] not in HISTORY_CLASSES:
        return False
    root = core.normalize_label(record.path[0])
    return (
        root in ALLOWED_ROOTS
        or root.startswith("history of ")
        or (include_localities and root == "united states local history")
    )


def named_person(label: str) -> bool:
    normalized = core.normalize_label(label)
    if PERSON_CONTEXT_RE.search(normalized) or core.PERSON_NAME_RE.match(label.strip()):
        return True
    match = DATED_PERSON_RE.match(label.strip())
    if not match:
        return False
    full_label = f" {normalized} "
    era_terms = {
        "administration", "administrations", "age", "campaign", "civil war", "conflict", "dynasty",
        "empire", "era", "national socialism", "occupation", "period", "regime",
        "republic", "revolution", "revolt", "war",
    }
    # Named administrations and conventionally named political periods remain
    # useful eras; bare sovereign/person headings are authority records.
    return not any(f" {term} " in full_label for term in era_terms)


def is_locality_route(labels: tuple[str, ...]) -> bool:
    return any(
        any(term in core.normalize_label(label) for term in LOCALITY_ROUTE_TERMS)
        for label in labels
    )


def locality_display_label(label: str) -> str | None:
    """Give useful visible names to LCC's locality filing containers."""
    normalized = core.normalize_label(label)
    if normalized in {
        "local history and description", "history and description",
        "history antiquities description",
    }:
        return "Local History & Places"
    if "cities towns" in normalized or "cities and towns" in normalized or normalized.startswith("individual cities"):
        return "Cities & Towns"
    if any(term in normalized for term in (
        "provinces regions", "regions provinces", "regions states territories",
        "regions counties", "by state province", "sections districts",
    )) or (normalized.startswith("regions") and core.FILING_RE.search(label)):
        return "Regions & Administrative Divisions"
    if normalized in {"localities", "individual localities"}:
        return "Localities"
    return None


def excluded_component(label: str, allow_localities: bool = False) -> str | None:
    normalized = core.normalize_label(label)
    filing_container = bool(
        core.FILING_RE.search(label) or core.ALPHA_BUCKET_RE.fullmatch(label.strip())
    )
    if filing_container:
        if allow_localities:
            return None
        return "alphabetic filing record"
    if not allow_localities and (core.is_geographic_container_caption(label) or normalized in {
        "cities towns etc", "local history and description", "localities",
        "history and description", "history antiquities description",
    } or normalized.startswith((
        "regions states", "regions provinces", "individual cities",
    )) or normalized == "provinces"):
        return "place/locality authority tree"
    if named_person(label):
        return "named-person or named-author authority tree"
    matched_authority_terms = [term for term in EXCLUDED_BRANCH_TERMS if term in normalized]
    if matched_authority_terms and not (
        allow_localities
        and all(term in LOCALITY_BRANCH_TERMS for term in matched_authority_terms)
    ):
        return "document, institution, or named-entity authority tree"
    if "institution of" in normalized or "university of" in normalized:
        return "document, institution, or named-entity authority tree"
    if normalized in {
        "artillery", "cavalry", "divisions", "engineers", "infantry",
        "marine corps", "squadrons",
    } or any(term in normalized for term in MILITARY_UNIT_TERMS) or UNIT_NUMBER_RE.match(label.strip()):
        return "military unit or formation tree"
    if core.is_form_caption(label) or any(term in normalized for term in FORM_TERMS):
        return "bibliographic or document-form tree"
    if normalized.startswith("description and travel"):
        return "description-and-travel tree"
    return None


def transparent(label: str, allow_localities: bool = False) -> bool:
    normalized = core.normalize_label(label)
    if allow_localities:
        if locality_display_label(label) is not None:
            return False
        if core.FILING_RE.search(label) or core.ALPHA_BUCKET_RE.fullmatch(label.strip()):
            return True
        if normalized == "local" or re.fullmatch(r"saint(?:e)? [a-z](?: saint(?:e)? [a-z])?", normalized):
            return True
        if "-" in label and (
            re.fullmatch(r"(?:la|le|les)[a-z]* (?:la|le|les)?[a-z]+", normalized)
            or re.fullmatch(r"(?:la|le|les) [a-z] (?:la|le|les) [a-z]", normalized)
        ):
            return True
    return (
        normalized in TRANSPARENT_LABELS
        or normalized.startswith("other ")
        or normalized.startswith("individual events")
        or normalized.startswith("individual battles")
        or normalized.startswith("individual campaigns")
        or normalized.startswith("individual wars")
        or normalized in {"individual states and empires", "individual empires"}
    )


def span_contains(parent: core.Record, child: core.Record) -> bool:
    return core.span_is_within(child.start, child.end, parent)


def build_manifest(
    taxonomy_path: Path,
    snapshot_path: Path,
    include_localities: bool = False,
) -> dict[str, object]:
    taxonomy = core.load_taxonomy(taxonomy_path)
    records, _children, _by_label, _by_span, _by_class, _sorted = core.load_snapshot(snapshot_path)
    concepts: dict[str, str] = taxonomy["concepts"]
    routes: dict[str, list[tuple[str, ...]]] = taxonomy["routes"]
    selectors: dict[str, list[tuple[str, str, int]]] = taxonomy["selectors"]
    labels: dict[str, list[tuple[str, int]]] = taxonomy["labels"]
    parents: dict[str, list[tuple[str, int, int]]] = taxonomy["parents"]

    history_ids = {cid for cid, values in routes.items() if any(is_history_route(route) for route in values)}
    span_concepts: dict[tuple[str, str], list[tuple[str, set[str]]]] = defaultdict(list)
    for cid in history_ids:
        names = {core.normalize_label(concepts[cid])}
        names.update(core.normalize_label(label) for label, _ in labels.get(cid, []))
        for system, selector, _ in selectors.get(cid, []):
            if system == "lcc" and (span := core.selector_span(selector)):
                span_concepts[span].append((cid, names))

    represented: dict[int, str] = {}
    for record in records.values():
        normalized = core.normalize_label(record.caption)
        for cid, names in span_concepts.get((record.start, record.end), []):
            if normalized in names or any(
                len(name) > 10 and (normalized.startswith(name) or name.startswith(normalized))
                for name in names
            ):
                represented[record.record_id] = cid
                break

    eligible = [
        record for record in records.values()
        if allowed_root(record, include_localities)
    ]
    duplicate_span_groups: dict[tuple[tuple[str, ...], str, str], list[core.Record]] = defaultdict(list)
    for record in eligible:
        duplicate_span_groups[(record.path, record.start, record.end)].append(record)
    ambiguous_span_records: set[int] = set()
    span_aliases: dict[int, int] = {}
    for group in duplicate_span_groups.values():
        if len(group) < 2:
            continue
        normalized = {
            record.record_id: core.normalize_label(record.caption) for record in group
        }
        canonical = min(group, key=lambda record: (len(normalized[record.record_id]), record.record_id))
        canonical_label = normalized[canonical.record_id]
        if all(
            label.startswith(canonical_label) or canonical_label.startswith(label)
            for label in normalized.values()
        ):
            for record in group:
                if record.record_id != canonical.record_id:
                    span_aliases[record.record_id] = canonical.record_id
        else:
            ambiguous_span_records.update(record.record_id for record in group)
    by_full_path: dict[tuple[str, ...], list[core.Record]] = defaultdict(list)
    for record in eligible:
        by_full_path[record.full_path].append(record)

    represented_eligible = {
        rid: cid for rid, cid in represented.items()
        if allowed_root(records[rid], include_localities)
    }
    assigned: dict[int, str] = dict(represented_eligible)
    new_nodes: dict[str, str] = {}
    new_edges: list[tuple[str, str]] = []
    desired_selectors: dict[str, set[str]] = defaultdict(set)
    desired_labels: dict[str, set[str]] = defaultdict(set)
    virtual_children: dict[tuple[str, str], list[str]] = defaultdict(list)
    for child, values in parents.items():
        for parent, _, _ in values:
            virtual_children[(parent, core.normalize_label(concepts[child]))].append(child)
            if include_localities and (mapped := locality_display_label(concepts[child])):
                virtual_children[(parent, core.normalize_label(mapped))].append(child)

    max_priority = {
        cid: max((core.selector_priority(value) for system, value, _ in values if system == "lcc"), default=4)
        for cid, values in selectors.items()
    }
    concept_record: dict[str, core.Record] = {}
    for rid, cid in represented_eligible.items():
        old = concept_record.get(cid)
        if old is None or len(records[rid].path) > len(old.path):
            concept_record[cid] = records[rid]

    excluded = Counter()
    excluded_examples: dict[str, list[str]] = defaultdict(list)
    collapsed_same_span = 0
    transparent_count = 0
    reused_children = 0

    def nearest_parent(record: core.Record) -> tuple[core.Record | None, str] | None:
        locality_record = include_localities and is_locality_route(record.full_path)
        for size in range(len(record.path), 0, -1):
            candidates = [
                ancestor for ancestor in by_full_path.get(record.path[:size], [])
                if ancestor.record_id in assigned
                and (span_contains(ancestor, record) or locality_record)
            ]
            if candidates:
                candidates.sort(key=lambda item: (len(item.path), item.record_id), reverse=True)
                ancestor = candidates[0]
                return ancestor, assigned[ancestor.record_id]
        root_anchor = ROOT_ANCHORS.get(core.normalize_label(record.path[0]))
        if root_anchor:
            return None, root_anchor
        return None

    def record_exclusion(record: core.Record) -> str | None:
        # Inspect the complete structural route. An already-represented record
        # inside a locality or unit schedule must not act as a back door that
        # makes the rest of that authority tree visible.
        normalized_route = [core.normalize_label(component) for component in record.full_path]
        if (
            normalized_route[0] == "history of civilization"
            and "civilization and race" in normalized_route
            and any(component in {
                "caucasian aryan", "nordic", "alpine celtic",
                "mediterranean latin", "black", "semitic",
            } for component in normalized_route)
        ):
            return "obsolete racial-classification tree"
        allow_locality_route = include_localities and is_locality_route(record.full_path)
        for component in record.full_path[1:]:
            if reason := excluded_component(component, allow_locality_route):
                return reason
        return None

    def add_exclusion(reason: str, record: core.Record) -> None:
        excluded[reason] += 1
        if len(excluded_examples[reason]) < 20:
            excluded_examples[reason].append(
                f"{record.start}..{record.end}: " + " / ".join(record.full_path)
            )

    # Structural ancestors are always encountered before descendants.
    eligible.sort(key=lambda item: (
        len(item.path), core.number_key(item.start) or (10**9, 0.0, ()), item.record_id
    ))
    for record in eligible:
        if record.record_id in assigned:
            continue
        if record.record_id in ambiguous_span_records:
            add_exclusion("ambiguous duplicate LCC span", record)
            continue
        if canonical_id := span_aliases.get(record.record_id):
            if canonical_id not in assigned:
                add_exclusion("unresolved duplicate-span alias", record)
                continue
            target = assigned[canonical_id]
            assigned[record.record_id] = target
            desired_labels[target].add(record.caption)
            continue
        found = nearest_parent(record)
        if found is None:
            add_exclusion("no represented History ancestor", record)
            continue
        ancestor, parent_id = found
        if reason := record_exclusion(record):
            add_exclusion(reason, record)
            continue

        if (
            include_localities
            and core.normalize_label(record.path[0]) == "united states local history"
            and len(record.path) == 1
            and (direct_anchor := US_LOCALITY_DIRECT_ANCHORS.get(core.normalize_label(record.caption)))
        ):
            assigned[record.record_id] = direct_anchor
            priority = min(9, max_priority.get(direct_anchor, 2) + 1)
            desired_selectors[direct_anchor].add(f"{record.start}..{record.end}@{priority}")
            desired_labels[direct_anchor].add(record.caption)
            max_priority[direct_anchor] = max(max_priority.get(direct_anchor, 0), priority)
            reused_children += 1
            continue

        # Structural catch-alls are traversed without becoming user-facing.
        allow_locality_route = include_localities and is_locality_route(record.full_path)
        if transparent(record.caption, allow_locality_route):
            assigned[record.record_id] = parent_id
            transparent_count += 1
            continue

        # LCC sometimes gives a child exactly the parent's number. It cannot
        # classify a book more narrowly, so retain only its source wording.
        if ancestor is not None and record.start == ancestor.start and record.end == ancestor.end:
            assigned[record.record_id] = parent_id
            desired_labels[parent_id].add(record.caption)
            collapsed_same_span += 1
            continue

        visible_label = locality_display_label(record.caption) if allow_locality_route else None
        preferred_label = visible_label or record.caption
        parent_label = new_nodes.get(parent_id, concepts.get(parent_id, ""))
        if core.normalize_label(preferred_label) == core.normalize_label(parent_label):
            assigned[record.record_id] = parent_id
            priority = min(9, max_priority.get(parent_id, 4) + 1)
            desired_selectors[parent_id].add(f"{record.start}..{record.end}@{priority}")
            desired_labels[parent_id].add(record.caption)
            max_priority[parent_id] = max(max_priority.get(parent_id, 0), priority)
            transparent_count += 1
            continue
        key = (parent_id, core.normalize_label(preferred_label))
        existing = virtual_children.get(key, [])
        if existing:
            target = existing[0]
            reused_children += 1
        else:
            target = "bokheim:history-lcc-full:" + str(uuid.uuid5(
                NAMESPACE, f"{parent_id}\n{core.normalize_label(record.caption)}"
            ))
            if target not in concepts and target not in new_nodes:
                new_nodes[target] = preferred_label
            new_edges.append((target, parent_id))
            virtual_children[key].append(target)

        priority = min(9, max_priority.get(parent_id, 4) + 1)
        desired_selectors[target].add(f"{record.start}..{record.end}@{priority}")
        desired_labels[target].add(record.caption)
        max_priority[target] = max(max_priority.get(target, 0), priority)
        assigned[record.record_id] = target
        concept_record[target] = record

    # Stable LCC order within every new sibling group.
    new_edges.sort(key=lambda edge: (
        edge[1],
        min(desired_selectors.get(edge[0], {"ZZZ"})).split("@")[0],
        new_nodes.get(edge[0], concepts.get(edge[0], edge[0])),
    ))
    return {
        "version": 1,
        "source": "Library of Congress MDSConnect Classification 2016 retrospective",
        "history_concept_count_before": len(history_ids),
        "represented_snapshot_records": len(represented_eligible),
        "eligible_snapshot_records": len(eligible),
        "assigned_snapshot_records": len(assigned),
        "collapsed_same_span_records": collapsed_same_span,
        "transparent_structural_records": transparent_count,
        "reused_direct_children": reused_children,
        "new_nodes": [
            {"concept_id": cid, "preferred_label": label}
            for cid, label in sorted(new_nodes.items())
        ],
        "edges": [
            {"child_id": child, "parent_id": parent} for child, parent in new_edges
        ],
        "selectors": [
            {"concept_id": cid, "selector": value}
            for cid in sorted(desired_selectors) for value in sorted(desired_selectors[cid])
        ],
        "labels": [
            {"concept_id": cid, "label": value}
            for cid in sorted(desired_labels) for value in sorted(desired_labels[cid])
        ],
        "excluded": {
            reason: {"count": count, "examples": excluded_examples[reason]}
            for reason, count in sorted(excluded.items())
        },
    }


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def render_sql(manifest: dict[str, object], taxonomy_path: Path) -> str:
    taxonomy = core.load_taxonomy(taxonomy_path)
    parents = taxonomy["parents"]
    selectors = taxonomy["selectors"]
    labels = taxonomy["labels"]
    existing_edges = {
        (child, parent) for child, values in parents.items() for parent, _, _ in values
    }
    existing_selectors = {
        cid: {value for system, value, _ in values if system == "lcc"}
        for cid, values in selectors.items()
    }
    existing_labels = {cid: {value for value, _ in values} for cid, values in labels.items()}
    lines = [
        "-- Exhaustive semantic LCC continuation beneath the curated History hierarchy.",
        "-- Catalog forms and authority/filing trees remain in the source snapshot only.",
        "-- Generated by scripts/generate-exhaustive-history-lcc-migration.py.",
        "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
    ]
    nodes = manifest["new_nodes"]
    if nodes:
        lines.append("INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES")
        lines += [
            f"    ({sql(row['concept_id'])},{sql(row['preferred_label'])})"
            f"{',' if index + 1 < len(nodes) else ';'}"
            for index, row in enumerate(nodes)
        ]
        lines.append("")

    next_ordinal = {
        cid: max((ordinal for _, ordinal, _ in values), default=-1) + 1
        for cid, values in parents.items()
    }
    next_display: dict[str, int] = defaultdict(int)
    # The children map does not carry display order, so derive it from parents.
    for child, values in parents.items():
        for parent, _, display in values:
            next_display[parent] = max(next_display[parent], display + 10)
    edge_rows = []
    for row in manifest["edges"]:
        child, parent = row["child_id"], row["parent_id"]
        if (child, parent) in existing_edges:
            continue
        ordinal = next_ordinal.get(child, 0)
        next_ordinal[child] = ordinal + 1
        display = next_display[parent]
        next_display[parent] += 10
        edge_rows.append((child, parent, ordinal, display))
    if edge_rows:
        lines.append("INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES")
        lines += [
            f"    ({sql(child)},{sql(parent)},{ordinal},{display})"
            f"{',' if index + 1 < len(edge_rows) else ';'}"
            for index, (child, parent, ordinal, display) in enumerate(edge_rows)
        ]
        lines.append("")

    next_selector = {
        cid: max((ordinal for _, _, ordinal in values), default=-1) + 1
        for cid, values in selectors.items()
    }
    selector_rows = []
    for row in manifest["selectors"]:
        cid, value = row["concept_id"], row["selector"]
        if value in existing_selectors.get(cid, set()):
            continue
        ordinal = next_selector.get(cid, 0)
        next_selector[cid] = ordinal + 1
        selector_rows.append((cid, value, ordinal))
    if selector_rows:
        lines.append("INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES")
        lines += [
            f"    ({sql(cid)},'lcc',{sql(value)},{ordinal})"
            f"{',' if index + 1 < len(selector_rows) else ';'}"
            for index, (cid, value, ordinal) in enumerate(selector_rows)
        ]
        lines.append("")

    next_label = {
        cid: max((ordinal for _, ordinal in values), default=-1) + 1
        for cid, values in labels.items()
    }
    label_rows = []
    for row in manifest["labels"]:
        cid, value = row["concept_id"], row["label"]
        if value in existing_labels.get(cid, set()):
            continue
        ordinal = next_label.get(cid, 0)
        next_label[cid] = ordinal + 1
        label_rows.append((cid, value, ordinal))
    if label_rows:
        lines.append("INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES")
        lines += [
            f"    ({sql(cid)},{sql(value)},{ordinal})"
            f"{',' if index + 1 < len(label_rows) else ';'}"
            for index, (cid, value, ordinal) in enumerate(label_rows)
        ]
        lines.append("")
    lines += ["COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--taxonomy", type=Path, default=Path("shared/subject-projection/data/unified-taxonomy-v2.sqlite3"))
    parser.add_argument("--snapshot", type=Path, default=Path("shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--manifest", type=Path, default=Path("shared/subject-projection/data/history-lcc-exhaustive-manifest.json"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_history_lcc_exhaustive.sql"))
    args = parser.parse_args()
    manifest = build_manifest(args.taxonomy, args.snapshot)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    args.output.write_text(render_sql(manifest, args.taxonomy), encoding="utf-8")
    print(json.dumps({
        "eligible_snapshot_records": manifest["eligible_snapshot_records"],
        "assigned_snapshot_records": manifest["assigned_snapshot_records"],
        "new_nodes": len(manifest["new_nodes"]),
        "edges": len(manifest["edges"]),
        "selectors": len(manifest["selectors"]),
        "collapsed_same_span_records": manifest["collapsed_same_span_records"],
        "transparent_structural_records": manifest["transparent_structural_records"],
        "excluded": {key: value["count"] for key, value in manifest["excluded"].items()},
    }, indent=2))


if __name__ == "__main__":
    main()
