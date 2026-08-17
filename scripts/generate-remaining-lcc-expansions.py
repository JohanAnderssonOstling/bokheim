#!/usr/bin/env python3
"""Freeze and generate all remaining eligible one-level LCC expansions.

The structural snapshot is used only at generation time.  A manifest freezes the
set of current leaf concepts so rerunning the generator after applying the SQL
does not recursively expand the newly-created leaves.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import re
import sqlite3
import unicodedata
import uuid
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path


NAMESPACE = uuid.UUID("16d1668e-f7d7-5b37-9818-ce024c7ce74b")
US_PRESIDENTIAL_ADMINISTRATIONS_ID = "bokheim:subject:history:united_states:presidential_administrations"
FILING_RE = re.compile(r"(?:^|[^a-z])a\s*[-–]\s*z(?:[^a-z]|$)", re.IGNORECASE)
NUMBER_RE = re.compile(r"^([A-Z]+)(\d+(?:\.\d+)?)(.*)$")
PERSON_NAME_RE = re.compile(r"^[^.;:()]{2,60},\s+(?:[A-ZÀ-ÖØ-Þ][^,.;:()]*|[A-Z]\.)")
ALPHA_BUCKET_RE = re.compile(r"^[A-Z](?:\s*[-–]\s*[A-Z][a-z]{0,4})?$")

# These captions describe editions, reference apparatus, audiences, or filing
# devices rather than subjects that belong in the visible taxonomy.
FORM_CAPTION_PREFIXES = (
    "addresses essays lectures", "almanacs", "bibliography", "biography",
    "collected works", "compends", "computer network resources", "congresses",
    "dictionaries", "directories", "encyclopedias", "general special",
    "general works", "guidebooks", "handbooks", "juvenile works", "manuals",
    "museums exhibitions", "pamphlets", "periodicals", "pictorial works",
    "popular works", "problems exercises examinations", "sources and documents",
    "special topics", "study and teaching", "terminology abbreviations",
)
GEOGRAPHIC_CONTAINER_PREFIXES = (
    "by region", "by country", "by state", "by province", "by locality",
    "cities and towns", "individual cities", "individual regions",
    "other countries", "other regions", "regions natural features",
    "territories administrative districts",
)
HISTORY_SUBJECT_TERMS = (
    "antiquit", "assimilation", "civilization", "diplomatic", "economic",
    "emancipation", "ethnograph", "foreign relations", "foreign and general relations",
    "foreign countries", "historical geography", "historiograph", "history", "identity", "intellectual life",
    "military", "naval", "period", "philosemit", "political", "population",
    "national characteristics", "revolution", "social life", "war", "zionism",
)
PARALLEL_LAW_SELECTOR_RE = re.compile(r"^KDZ0\.\.KH", re.IGNORECASE)
PARENT_RECORD_SPAN_OVERRIDES = {
    # The outline contains a typo (BG rather than BF) and two ranges that cover
    # only a parent's children.  The structural snapshot has the parent spans.
    "unified:3597ca98-e662-5fac-9e5b-56965fe74c85": ("BF1068", "BF1073"),
    "unified:c55f73d2-8a94-535d-807b-dcb47c3cc629": ("NC1900", "NC1900"),
    "unified:ea94b06c-41e5-5e61-99bc-0cdeed3da068": ("QR171.A1", "QR171.Z"),
    # The imported outline selectors for these two Ukraine leaves lost their
    # decimal points (DK508.42 and DK508.442 became DK5091 and DK5109-series
    # ranges).  Their taxonomy routes and captions map unambiguously to these
    # structural snapshot records.  The History end below preserves the
    # snapshot's own retrospective typo so the exact source record is selected.
    "unified:f179a7c5-53a9-5f7b-a4e5-24eb19e31417": ("DK508.42", "DK508.44"),
    "unified:b5e23bd2-8b1c-5146-8b9c-8e3483d68eac": ("DK508.442", "DK503.851"),
    # The outline points at the parent number while the structural record uses
    # its full cutter span.  Its immediate records are filing/form divisions,
    # so this alignment intentionally classifies it as non-expandable.
    "unified:d1418c47-d2f1-54da-8545-e734b4c25579": ("GC63.A1", "GC63.Z"),
}
# These two outline leaves intentionally collapse a parent range together with
# one or more named subranges.  Expanding every matched subrange is necessary to
# expose the approved groundwater and landform subjects in one visible step.
COLLAPSED_MULTI_RECORD_CONCEPTS = {
    "unified:628a666e-c580-599d-a670-8ae43ecd0e29",  # Geomorphology
    "unified:8d146cce-4618-5d19-ba1d-e73783d3e668",  # Hydrology
}
PARALLEL_LAW_CAPTION_ALIASES = {
    "unified:fa321b44-5ec4-5579-91b9-eefb87c379ed": (
        "Alcohol. Alcoholic beverages. Liquor laws",
        "Alcohol. Alcoholic beverages. Prohibition. Liquor laws",
        "Liquor laws",
    ),
    "unified:ab689681-185b-52c0-9f02-b5f3b2683df6": ("Business associations",),
    "unified:71055030-4d02-53ec-ab79-a44bb5e9d40e": ("Economic law",),
    "unified:2ed874ce-a6cd-5ceb-a16b-299b52ae228d": (
        "Fisheries", "Fisheries and fishing. Pêche", "Fishery",
    ),
    "unified:145194f0-68e1-52c1-8c33-610d2dc22866": (
        "Public welfare. Public assistance",
        "Public welfare. Public assistance. Poor laws",
        "Public welfare. Public assistance. Private charities",
    ),
    "unified:6d8c112e-3f69-5729-ab2d-71bd8491d77d": (
        "Quasi contracts", "Restitution. Quasi contracts. Unjust enrichment",
    ),
    "unified:b6e447fc-e40d-5fc4-a85b-7b8ce269a842": (
        "Form and structure of government", "Form and structure of governments",
        "Structure of government. Federal and state relations. Jurisdiction",
        "Structure of government. Federal-provincial relations. Interprovincial relations. Jurisdiction",
        "Structure of government. Federal-provincial relations. Jurisdiction",
        "Structure of government. Jurisdiction",
    ),
    "unified:34163efe-863b-51c2-b699-75630ead96a2": (
        "Tariff. Custom duties", "Tariff. Customs duties",
        "Tariff. Trade agreements. Customs", "Tariff. Trade agreements. Customs duties",
    ),
    "unified:60489245-0ccc-539f-bee4-f639d8791f09": (
        "Trade regulation. Control of trade practice. Consumer protection",
        "Trade regulation. Control of trade practices. Consumer protection",
        "Trade regulations. Control of trade practices. Consumer protection",
    ),
}


@dataclass(frozen=True)
class Record:
    record_id: int
    control_number: str
    caption: str
    start: str
    end: str
    path: tuple[str, ...]

    @property
    def full_path(self) -> tuple[str, ...]:
        return self.path + (self.caption,)


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def normalize_label(value: str) -> str:
    value = unicodedata.normalize("NFKD", value).encode("ascii", "ignore").decode()
    value = value.casefold().replace("&", " and ")
    return " ".join(re.findall(r"[a-z0-9]+", value))


def label_tokens(value: str) -> set[str]:
    return {token for token in normalize_label(value).split() if len(token) > 2}


def restore_end(start: str, end: str | None) -> str:
    # A few retrospective MDSConnect spans contain a terminal full stop after
    # the classification number. It is record punctuation, not part of an LCC
    # number, and would otherwise produce an invalid selector such as
    # ``DK501.92...DK501.92.``.
    start = start.strip().rstrip(".")
    if not end:
        return start
    end = end.strip().rstrip(".").replace("..", ".")
    base = re.match(r"^([A-Z]+\d+(?:\.\d+)?)", start)
    if re.fullmatch(r"[A-Z]+", end) and base:
        return base.group(1) + "." + end
    if end.startswith(".") and base:
        return base.group(1) + end
    if re.match(r"^[A-Z]{1,4}\s*'?\d", end):
        return end
    if NUMBER_RE.match(end):
        return end
    letters = re.match(r"^[A-Z]+", start)
    return (letters.group(0) if letters else "") + end


def strip_priority(selector: str) -> str:
    return selector.rsplit("@", 1)[0] if "@" in selector else selector


def selector_priority(selector: str) -> int:
    if "@" not in selector:
        return 0
    suffix = selector.rsplit("@", 1)[1]
    return int(suffix) if suffix.isdigit() else 0


def selector_span(selector: str) -> tuple[str, str] | None:
    base = strip_priority(selector)
    if ".." in base:
        start, end = base.split("..", 1)
        return start, restore_end(start, end)
    if re.fullmatch(r"[A-Z]+", base):
        return None
    if NUMBER_RE.match(base):
        return base, base
    return None


def number_key(value: str) -> tuple[int, float, tuple[tuple[int, object], ...]] | None:
    match = NUMBER_RE.match(value)
    if not match:
        return None
    letters, number, suffix = match.groups()
    letter_key = 0
    for char in letters:
        letter_key = letter_key * 27 + ord(char) - 64
    parts: list[tuple[int, object]] = []
    for token in re.findall(r"[A-Z]+|\d+(?:\.\d+)?|[^A-Z0-9]+", suffix):
        if token[0].isdigit():
            parts.append((0, float(token)))
        elif token[0].isalpha():
            parts.append((1, token))
        else:
            parts.append((2, token))
    return letter_key, float(number), tuple(parts)


def class_prefix(value: str) -> str:
    match = re.match(r"^[A-Z]+", value)
    return match.group(0) if match else ""


def span_is_within(start: str, end: str, parent: Record) -> bool:
    start_key, end_key = number_key(start), number_key(end)
    parent_start, parent_end = number_key(parent.start), number_key(parent.end)
    return bool(
        start_key and end_key and parent_start and parent_end
        and parent_start <= start_key <= end_key <= parent_end
    )


def enforce_structural_span_containment(raw_records: list[Record]) -> list[Record]:
    """Repair or reject cross-class spans that escape their structural parent.

    The retrospective snapshot contains occasional one-letter transcription
    errors in either the start or end class (for example GC118-QC118.2 beneath
    the GC109-GC149 Chemical oceanography parent).  A legitimate cross-subclass
    schedule is retained when it is contained by its parent.  An escaping span
    is repaired only when replacing one class prefix makes exactly one candidate
    fit; otherwise the unsafe record is omitted from visible generation.
    """
    raw_by_full_path: dict[tuple[str, ...], list[Record]] = defaultdict(list)
    for record in raw_records:
        raw_by_full_path[record.full_path].append(record)

    accepted: dict[int, Record] = {}
    for record in sorted(raw_records, key=lambda item: (len(item.path), item.record_id)):
        parents = [
            accepted[parent.record_id]
            for parent in raw_by_full_path.get(record.path, [])
            if parent.record_id in accepted
        ]
        start_class, end_class = class_prefix(record.start), class_prefix(record.end)
        if not parents or start_class == end_class or any(
            span_is_within(record.start, record.end, parent) for parent in parents
        ):
            accepted[record.record_id] = record
            continue

        candidates = {
            (record.start, re.sub(r"^[A-Z]+", start_class, record.end)),
            (re.sub(r"^[A-Z]+", end_class, record.start), record.end),
        }
        valid = {
            candidate for candidate in candidates
            if candidate[0] != candidate[1]
            and any(span_is_within(candidate[0], candidate[1], parent) for parent in parents)
        }
        if len(valid) != 1:
            continue
        start, end = valid.pop()
        accepted[record.record_id] = Record(
            record.record_id, record.control_number, record.caption,
            start, end, record.path,
        )
    return list(accepted.values())


def range_relation(selector: str, record: Record) -> int:
    """Return a high score for exact/overlapping selector and record ranges."""
    base = strip_priority(selector)
    if re.fullmatch(r"[A-Z]+", base):
        return 1_000 if record.start.startswith(base) else 0
    span = selector_span(selector)
    if not span:
        return 0
    start, end = span
    if start == record.start and end == record.end:
        return 10_000
    sk, ek = number_key(start), number_key(end)
    rsk, rek = number_key(record.start), number_key(record.end)
    if not all((sk, ek, rsk, rek)):
        return 0
    if sk <= rsk <= ek or sk <= rek <= ek or rsk <= sk <= rek:
        return 5_000
    return 0


def route_score(snapshot_path: tuple[str, ...], taxonomy_routes: list[tuple[str, ...]]) -> int:
    best = 0
    snapshot_norm = [normalize_label(label) for label in snapshot_path]
    snapshot_tokens = set().union(*(label_tokens(label) for label in snapshot_path)) if snapshot_path else set()
    for route in taxonomy_routes:
        ancestors = route[:-1]
        route_norm = [normalize_label(label) for label in ancestors]
        suffix = 0
        for left, right in zip(reversed(snapshot_norm), reversed(route_norm)):
            if left != right:
                break
            suffix += 1
        route_tokens = set().union(*(label_tokens(label) for label in ancestors)) if ancestors else set()
        score = suffix * 500 + len(snapshot_tokens & route_tokens) * 12
        if ancestors and snapshot_path:
            score += len(label_tokens(ancestors[-1]) & label_tokens(snapshot_path[-1])) * 60
        best = max(best, score)
    return best


def load_snapshot(path: Path) -> tuple[
    dict[int, Record], dict[tuple[str, ...], list[Record]], dict[str, list[Record]],
    dict[tuple[str, str], list[Record]], dict[str, list[Record]], list[str],
]:
    with sqlite3.connect(path) as db:
        hierarchy: dict[int, list[str]] = defaultdict(list)
        for record_id, _ordinal, label in db.execute(
            "SELECT record_id,ordinal,label FROM caption_hierarchy ORDER BY record_id,ordinal"
        ):
            hierarchy[record_id].append(label)
        rows = db.execute(
            """
            SELECT cr.record_id,cr.control_number,cr.caption,sp.start_number,
                   coalesce(sp.end_number,sp.start_number)
            FROM classification_record cr
            JOIN classification_span sp ON sp.record_id=cr.record_id AND sp.ordinal=0
            WHERE cr.caption IS NOT NULL AND trim(cr.caption)<>''
            """
        ).fetchall()
    raw_records = [
        Record(
            record_id,
            control,
            caption,
            start.strip().rstrip("."),
            restore_end(start, end),
            tuple(hierarchy.get(record_id, [])),
        )
        for record_id, control, caption, start, end in rows
    ]
    records: dict[int, Record] = {}
    children: dict[tuple[str, ...], list[Record]] = defaultdict(list)
    by_label: dict[str, list[Record]] = defaultdict(list)
    by_span: dict[tuple[str, str], list[Record]] = defaultdict(list)
    by_class1: dict[str, list[Record]] = defaultdict(list)
    for record in enforce_structural_span_containment(raw_records):
        record_id = record.record_id
        records[record_id] = record
        children[record.path].append(record)
        by_label[normalize_label(record.caption)].append(record)
        by_span[(record.start, record.end)].append(record)
        by_class1[record.start[:1]].append(record)
    for values in children.values():
        values.sort(key=lambda record: number_key(record.start) or (10**9, 0.0, ()))
    return records, children, by_label, by_span, by_class1, sorted(by_label)


def load_taxonomy(path: Path) -> dict[str, object]:
    with sqlite3.connect(path) as db:
        concepts = dict(db.execute("SELECT concept_id,preferred_label FROM concept"))
        parents: dict[str, list[tuple[str, int, int]]] = defaultdict(list)
        children: dict[str, list[str]] = defaultdict(list)
        for concept_id, parent_id, ordinal, display_order in db.execute(
            "SELECT concept_id,parent_concept_id,ordinal,display_order FROM concept_parent"
        ):
            parents[concept_id].append((parent_id, ordinal, display_order))
            children[parent_id].append(concept_id)
        selectors: dict[str, list[tuple[str, str, int]]] = defaultdict(list)
        for concept_id, system, selector, ordinal in db.execute(
            "SELECT concept_id,system_id,selector,ordinal FROM source_selector"
        ):
            selectors[concept_id].append((system, selector, ordinal))
        labels: dict[str, list[tuple[str, int]]] = defaultdict(list)
        for concept_id, label, ordinal in db.execute("SELECT concept_id,label,ordinal FROM source_label"):
            labels[concept_id].append((label, ordinal))
    route_cache: dict[str, list[tuple[str, ...]]] = {}

    def routes(concept_id: str, seen: frozenset[str] = frozenset()) -> list[tuple[str, ...]]:
        if concept_id in route_cache:
            return route_cache[concept_id]
        if concept_id in seen:
            raise SystemExit(f"cycle in taxonomy at {concept_id}")
        own = concepts[concept_id]
        if not parents.get(concept_id):
            result = [(own,)]
        else:
            result = [route + (own,) for parent_id, _, _ in parents[concept_id] for route in routes(parent_id, seen | {concept_id})]
        route_cache[concept_id] = result
        return result

    for concept_id in concepts:
        routes(concept_id)
    return {
        "concepts": concepts,
        "parents": parents,
        "children": children,
        "selectors": selectors,
        "labels": labels,
        "routes": route_cache,
    }


def exclusion_reason(routes: list[tuple[str, ...]]) -> str | None:
    normalized_routes = [" / ".join(normalize_label(label) for label in route) for route in routes]
    leaf_labels = {normalize_label(route[-1]) for route in routes}
    if any("individual author" in route for route in normalized_routes):
        return "individual-author route"
    if any("subject bibliography" in route for route in normalized_routes):
        return "contextual subject-bibliography endpoint"
    institution_terms = (
        "individual institutions", "individual universities", "individual colleges",
        "individual schools", "individual libraries", "individual museums",
    )
    if any(any(term in route for term in institution_terms) for route in normalized_routes):
        return "individual-institution authority route"
    geographic_containers = (
        "by region or country", "by country", "by state", "by province", "by locality",
        "other regions or countries", "other regions and countries",
    )
    if any(any(label == term or label.startswith(term + " ") for term in geographic_containers) for label in leaf_labels):
        return "geographic subdivision authority container"
    if "local history and description" in leaf_labels:
        return "regional-history locality authority container"
    return None


def is_form_caption(caption: str) -> bool:
    normalized = normalize_label(caption)
    if normalized in {"by subject", "collective", "general"} or normalized.startswith("by name of ") or (
        normalized.startswith("general ") and not normalized.startswith("general agreement")
    ):
        return True
    return any(normalized.startswith(prefix) for prefix in FORM_CAPTION_PREFIXES)


def is_geographic_container_caption(caption: str) -> bool:
    normalized = normalize_label(caption)
    if normalized == "local history and description":
        return True
    return any(normalized.startswith(prefix) for prefix in GEOGRAPHIC_CONTAINER_PREFIXES)


def is_history_subject_caption(caption: str, leaf_label: str) -> bool:
    normalized = normalize_label(caption)
    if normalize_label(leaf_label) == "description and travel":
        return bool(re.search(r"\d", caption)) or normalized.startswith("through ")
    if re.search(r"\d", caption):
        return True
    if normalized in {"ancient", "medieval", "modern", "by period", "description and travel", "geography"}:
        return True
    return any(term in normalized for term in HISTORY_SUBJECT_TERMS)


def is_us_presidential_administration(routes: list[tuple[str, ...]], caption: str) -> bool:
    normalized = normalize_label(caption)
    if "administration" not in normalized and "harding coolidge hoover era" not in normalized:
        return False
    return any(
        "humanities / history / history by region / americas / north america / united states / historical eras"
        in " / ".join(normalize_label(label) for label in route)
        for route in routes
    )


def child_exclusion_reason(routes: list[tuple[str, ...]], record: Record) -> str | None:
    if FILING_RE.search(record.caption):
        return "alphabetic filing record"
    normalized_routes = [" / ".join(normalize_label(label) for label in route) for route in routes]
    is_regional_history = any("humanities / history / history by region" in route for route in normalized_routes)
    is_geography = any(route.startswith("geography places and travel /") for route in normalized_routes)
    is_law = any(" / law /" in f" / {route} /" for route in normalized_routes)
    manual_recovery = any(
        normalize_label(route[-1]) in {
            "sonambulism", "copying enlarging and reduction of drawings",
            "microorganisms in the animal body",
        }
        for route in routes
    )
    if (is_regional_history or is_geography or is_law or manual_recovery) and is_form_caption(record.caption):
        return "bibliographic or form subdivision"
    if (is_regional_history or is_geography) and is_geographic_container_caption(record.caption):
        return "geographic authority subdivision"
    if is_geography and any(" / maps /" in f" / {route} /" or " / atlases /" in f" / {route} /" for route in normalized_routes):
        return "map or atlas place/format subdivision"
    if is_regional_history and not any(
        is_history_subject_caption(record.caption, route[-1]) for route in routes
        if "humanities / history / history by region" in " / ".join(normalize_label(label) for label in route)
    ):
        return "regional-history locality or non-subject record"
    is_literature = any(" / literature /" in f" / {route} /" for route in normalized_routes)
    if is_literature and record.start.startswith("P"):
        if ALPHA_BUCKET_RE.fullmatch(record.caption.strip()):
            return "alphabetic author bucket"
        if PERSON_NAME_RE.match(record.caption.strip()):
            return "named individual author"
    return None


def choose_parent_records(
    concept_id: str,
    taxonomy: dict[str, object],
    records_by_label: dict[str, list[Record]],
    records_by_span: dict[tuple[str, str], list[Record]],
    records_by_class1: dict[str, list[Record]],
    sorted_snapshot_labels: list[str],
) -> tuple[list[Record], str | None]:
    concepts: dict[str, str] = taxonomy["concepts"]  # type: ignore[assignment]
    labels: dict[str, list[tuple[str, int]]] = taxonomy["labels"]  # type: ignore[assignment]
    selectors: dict[str, list[tuple[str, str, int]]] = taxonomy["selectors"]  # type: ignore[assignment]
    routes: dict[str, list[tuple[str, ...]]] = taxonomy["routes"]  # type: ignore[assignment]
    candidate_labels = {concepts[concept_id], *(label for label, _ in labels.get(concept_id, []))}
    law_aliases = set(PARALLEL_LAW_CAPTION_ALIASES.get(concept_id, ()))
    candidates = {
        record.record_id: record
        for label in candidate_labels | law_aliases
        for record in records_by_label.get(normalize_label(label), [])
    }
    lcc_selectors = [selector for system, selector, _ in selectors.get(concept_id, []) if system == "lcc"]
    wanted_tokens = set().union(*(label_tokens(label) for label in candidate_labels))
    # Outline labels are sometimes truncated or modernized.  Prefix caption
    # matches and exact classification spans recover those records without
    # relying on a globally ambiguous caption.
    for label in candidate_labels:
        normalized = normalize_label(label)
        if len(normalized) < 12:
            continue
        index = bisect.bisect_left(sorted_snapshot_labels, normalized)
        while index < len(sorted_snapshot_labels) and sorted_snapshot_labels[index].startswith(normalized):
            for record in records_by_label[sorted_snapshot_labels[index]]:
                candidates[record.record_id] = record
            index += 1
    for selector in lcc_selectors:
        span = selector_span(selector)
        if span:
            for record in records_by_span.get(span, []):
                candidates[record.record_id] = record
    override_span = PARENT_RECORD_SPAN_OVERRIDES.get(concept_id)
    if override_span:
        for record in records_by_span.get(override_span, []):
            candidates[record.record_id] = record
        override_records = records_by_span.get(override_span, [])
        if override_records:
            return sorted(override_records, key=lambda record: record.record_id), None
    if not candidates:
        first_letters = {strip_priority(selector)[:1] for selector in lcc_selectors}
        for first_letter in first_letters:
            for record in records_by_class1.get(first_letter, []):
                record_tokens = label_tokens(record.caption)
                if wanted_tokens and len(wanted_tokens & record_tokens) >= min(2, len(wanted_tokens)):
                    candidates[record.record_id] = record
    needs_range_fallback = not any(
        range_relation(selector, record)
        for selector in lcc_selectors
        for record in candidates.values()
    )
    chosen: dict[int, Record] = {}
    for selector in lcc_selectors:
        scored = []
        for record in candidates.values():
            relation = range_relation(selector, record)
            if relation:
                scored.append((relation + route_score(record.path, routes[concept_id]), record))
        if not scored and needs_range_fallback:
            first_letter = strip_priority(selector)[:1]
            for record in records_by_class1.get(first_letter, []):
                relation = range_relation(selector, record)
                if not relation:
                    continue
                overlap = wanted_tokens & label_tokens(record.caption)
                if overlap:
                    scored.append((
                        relation + route_score(record.path, routes[concept_id]) + len(overlap) * 80,
                        record,
                    ))
        if not scored:
            continue
        scored.sort(key=lambda item: (item[0], item[1].record_id), reverse=True)
        top_score = scored[0][0]
        # Keep exact ties only; they represent genuinely repeated spans.  Broad
        # selectors otherwise choose the route-compatible record.
        for score, record in scored:
            if score != top_score:
                break
            chosen[record.record_id] = record
    if not chosen and any(PARALLEL_LAW_SELECTOR_RE.match(strip_priority(selector)) for selector in lcc_selectors):
        exact_labels = {normalize_label(label) for label in candidate_labels | law_aliases}
        for record in candidates.values():
            normalized_path = " / ".join(normalize_label(label) for label in record.path)
            if (
                normalize_label(record.caption) in exact_labels
                and record.start.startswith("K")
                and "law" in normalized_path
                and not any(FILING_RE.search(label) for label in record.path)
            ):
                chosen[record.record_id] = record
        if chosen:
            # These aggregate outline selectors represent a topic's position in
            # parallel jurisdiction schedules, not a literal KDZ-to-KH range.
            return sorted(chosen.values(), key=lambda record: record.record_id), None
    if not chosen:
        return [], "no route-and-range parent match"
    safe = [
        record for record in chosen.values()
        if max((range_relation(selector, record) for selector in lcc_selectors), default=0) >= 5_000
        or route_score(record.path, routes[concept_id]) >= 60
    ]
    if not safe:
        return [], "no sufficiently confident route-and-range parent match"
    if concept_id in COLLAPSED_MULTI_RECORD_CONCEPTS:
        return sorted(safe, key=lambda record: record.record_id), None
    # A collapsed outline concept can carry selectors for both a structural
    # parent and several of its children.  Only expand the highest matched
    # record, otherwise grandchildren would be flattened into the same level.
    highest = [
        record for record in safe
        if not any(
            len(other.full_path) < len(record.full_path)
            and record.full_path[:len(other.full_path)] == other.full_path
            for other in safe
        )
    ]
    return sorted(highest, key=lambda record: record.record_id), None


def selector_for(record: Record) -> str:
    return f"{record.start}..{record.end}@4"


def build_manifest(taxonomy_path: Path, snapshot_path: Path) -> dict[str, object]:
    taxonomy = load_taxonomy(taxonomy_path)
    records, child_index, records_by_label, records_by_span, records_by_class1, sorted_snapshot_labels = load_snapshot(snapshot_path)
    concepts: dict[str, str] = taxonomy["concepts"]  # type: ignore[assignment]
    parents: dict[str, list[tuple[str, int, int]]] = taxonomy["parents"]  # type: ignore[assignment]
    current_children: dict[str, list[str]] = taxonomy["children"]  # type: ignore[assignment]
    selectors: dict[str, list[tuple[str, str, int]]] = taxonomy["selectors"]  # type: ignore[assignment]
    labels: dict[str, list[tuple[str, int]]] = taxonomy["labels"]  # type: ignore[assignment]
    routes: dict[str, list[tuple[str, ...]]] = taxonomy["routes"]  # type: ignore[assignment]

    frozen_leaves = sorted(
        concept_id for concept_id in concepts
        if not current_children.get(concept_id)
        and any(system == "lcc" for system, _, _ in selectors.get(concept_id, []))
        and max(
            (selector_priority(selector) for system, selector, _ in selectors.get(concept_id, []) if system == "lcc"),
            default=0,
        ) <= 3
    )

    # Exact existing LCC ranges are safe reuse candidates only when their label
    # also agrees with the snapshot caption.
    exact_existing: dict[tuple[str, str, str], list[str]] = defaultdict(list)
    for concept_id, concept_selectors in selectors.items():
        concept_labels = {normalize_label(concepts[concept_id]), *(normalize_label(label) for label, _ in labels.get(concept_id, []))}
        for system, selector, _ in concept_selectors:
            if system != "lcc":
                continue
            span = selector_span(selector)
            if not span:
                continue
            for label in concept_labels:
                exact_existing[(label, span[0], span[1])].append(concept_id)
    presidential_children = current_children.get(US_PRESIDENTIAL_ADMINISTRATIONS_ID, [])

    handled = []
    excluded = []
    unresolved = []
    non_expandable = []
    excluded_child_counts: dict[str, int] = defaultdict(int)
    excluded_child_examples: dict[str, list[str]] = defaultdict(list)
    child_specs = []
    for concept_id in frozen_leaves:
        all_labels = {concepts[concept_id], *(label for label, _ in labels.get(concept_id, []))}
        if any(FILING_RE.search(label) for label in all_labels):
            excluded.append({"concept_id": concept_id, "label": concepts[concept_id], "reason": "alphabetic filing parent"})
            continue
        reason = exclusion_reason(routes[concept_id])
        if reason:
            excluded.append({"concept_id": concept_id, "label": concepts[concept_id], "reason": reason})
            continue
        parent_records, error = choose_parent_records(
            concept_id, taxonomy, records_by_label, records_by_span,
            records_by_class1, sorted_snapshot_labels,
        )
        if error:
            excluded.append({
                "concept_id": concept_id,
                "label": concepts[concept_id],
                "reason": "no unambiguous route-and-range structural match",
            })
            continue
        semantic_children: dict[str, list[Record]] = defaultdict(list)
        for parent_record in parent_records:
            for child in child_index.get(parent_record.full_path, []):
                child_reason = child_exclusion_reason(routes[concept_id], child)
                if child_reason:
                    excluded_child_counts[child_reason] += 1
                    if len(excluded_child_examples[child_reason]) < 20:
                        excluded_child_examples[child_reason].append(child.caption)
                    continue
                semantic_children[normalize_label(child.caption)].append(child)
        if not semantic_children:
            non_expandable.append({
                "concept_id": concept_id,
                "label": concepts[concept_id],
                "snapshot_parent_record_ids": [record.record_id for record in parent_records],
                "reason": "no semantic immediate children after filing exclusions",
            })
            continue
        parent_entry = {
            "concept_id": concept_id,
            "label": concepts[concept_id],
            "snapshot_parent_record_ids": [record.record_id for record in parent_records],
            "semantic_child_count": sum(len(values) for values in semantic_children.values()),
        }
        handled.append(parent_entry)
        for normalized, child_records in sorted(semantic_children.items()):
            child_records.sort(key=lambda record: number_key(record.start) or (10**9, 0.0, ()))
            caption = child_records[0].caption
            reuse_options = []
            for record in child_records:
                reuse_options.extend(exact_existing.get((normalized, record.start, record.end), []))
            reuse_options = sorted({option for option in reuse_options if option != concept_id})
            target_id = reuse_options[0] if len(reuse_options) == 1 else None
            if target_id is None and is_us_presidential_administration(routes[concept_id], caption):
                child_normalized = normalize_label(caption)
                presidential_matches = [
                    candidate for candidate in presidential_children
                    if child_normalized == normalize_label(concepts[candidate])
                    or child_normalized.endswith(normalize_label(concepts[candidate]))
                    or (
                        len(label_tokens(concepts[candidate])) >= 2
                        and label_tokens(concepts[candidate]) <= label_tokens(caption)
                    )
                ]
                if len(presidential_matches) == 1:
                    target_id = presidential_matches[0]
            if target_id is None:
                target_id = "bokheim:lcc-detail:" + str(uuid.uuid5(NAMESPACE, f"{concept_id}\n{normalized}"))
            visible_parent_id = (
                US_PRESIDENTIAL_ADMINISTRATIONS_ID
                if is_us_presidential_administration(routes[concept_id], caption)
                else concept_id
            )
            child_specs.append({
                "parent_id": visible_parent_id,
                "source_parent_id": concept_id,
                "parent_label": concepts[concept_id],
                "target_id": target_id,
                "preferred_label": caption,
                "reused": target_id in concepts,
                "records": [
                    {"record_id": record.record_id, "control_number": record.control_number,
                     "caption": record.caption, "start": record.start, "end": record.end}
                    for record in child_records
                ],
            })

    digest = hashlib.sha256(taxonomy_path.read_bytes()).hexdigest()
    return {
        "version": 1,
        "taxonomy_sha256_before": digest,
        "frozen_lcc_leaf_count": len(frozen_leaves),
        "handled_parents": handled,
        "excluded_parents": excluded,
        "unresolved_parents": unresolved,
        "non_expandable_parents": non_expandable,
        "excluded_child_records": {
            reason: {"count": count, "examples": excluded_child_examples[reason]}
            for reason, count in sorted(excluded_child_counts.items())
        },
        "children": child_specs,
        "additional_edges": [
            {"child_id": child_id, "parent_id": US_PRESIDENTIAL_ADMINISTRATIONS_ID}
            for child_id in (
                "unified:461987d3-c50d-573e-abc2-57c309acd110",
                "unified:18493c17-3bb8-5768-875a-605b6705b9ea",
            )
            if child_id in concepts and US_PRESIDENTIAL_ADMINISTRATIONS_ID in concepts
        ],
    }


def render_sql(manifest: dict[str, object], taxonomy_path: Path) -> str:
    taxonomy = load_taxonomy(taxonomy_path)
    concepts: dict[str, str] = taxonomy["concepts"]  # type: ignore[assignment]
    parents: dict[str, list[tuple[str, int, int]]] = taxonomy["parents"]  # type: ignore[assignment]
    selectors: dict[str, list[tuple[str, str, int]]] = taxonomy["selectors"]  # type: ignore[assignment]
    labels: dict[str, list[tuple[str, int]]] = taxonomy["labels"]  # type: ignore[assignment]
    children: list[dict[str, object]] = manifest["children"]  # type: ignore[assignment]

    new_concepts: dict[str, str] = {}
    edge_pairs: set[tuple[str, str]] = set()
    desired_selectors: dict[str, set[str]] = defaultdict(set)
    desired_labels: dict[str, set[str]] = defaultdict(set)
    edge_order: dict[tuple[str, str], int] = {}
    order_by_parent: dict[str, int] = defaultdict(int)
    for spec in children:
        target = str(spec["target_id"])
        parent = str(spec["parent_id"])
        if target not in concepts:
            new_concepts[target] = str(spec["preferred_label"])
        edge_pairs.add((target, parent))
        order_by_parent[parent] += 10
        edge_order[(target, parent)] = order_by_parent[parent]
        for record in spec["records"]:  # type: ignore[union-attr]
            record = dict(record)
            desired_selectors[target].add(f"{record['start']}..{record['end']}@4")
            desired_labels[target].add(str(record["caption"]))
    for additional in manifest.get("additional_edges", []):  # type: ignore[union-attr]
        child = str(additional["child_id"])
        parent = str(additional["parent_id"])
        edge_pairs.add((child, parent))
        order_by_parent[parent] += 10
        edge_order[(child, parent)] = order_by_parent[parent]

    existing_edges = {(child, parent) for child, values in parents.items() for parent, _, _ in values}
    existing_selectors = {
        concept_id: {selector for system, selector, _ in values if system == "lcc"}
        for concept_id, values in selectors.items()
    }
    existing_labels = {concept_id: {label for label, _ in values} for concept_id, values in labels.items()}

    lines = [
        "-- Bulk one-level expansion of the frozen remaining eligible LCC leaves.",
        "-- Named-author, named-institution, regional-place, geographic-place, and A-Z filing structures are excluded.",
        "-- Jurisdiction-specific law schedules are retained as contextual concepts.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-remaining-lcc-expansions.py from the frozen manifest.",
        "-- Priority 4 expands the original priority-3 leaves while preserving narrower curated priority-4/5 mappings.",
        "-- This migration is intentionally idempotent.", "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
    ]
    if new_concepts:
        lines.append("INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES")
        rows = sorted(new_concepts.items())
        lines += [f"    ({sql(cid)},{sql(label)}){',' if i+1<len(rows) else ';'}" for i,(cid,label) in enumerate(rows)]
        lines.append("")

    new_edges = sorted(edge_pairs - existing_edges, key=lambda pair: (pair[1], edge_order[pair], pair[0]))
    if new_edges:
        next_ordinal = {concept_id: max((ordinal for _, ordinal, _ in values), default=-1) + 1 for concept_id, values in parents.items()}
        edge_rows = []
        for child, parent in new_edges:
            ordinal = next_ordinal.get(child, 0)
            next_ordinal[child] = ordinal + 1
            edge_rows.append((child, parent, ordinal, edge_order[(child, parent)]))
        lines.append("INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES")
        lines += [f"    ({sql(c)},{sql(p)},{o},{d}){',' if i+1<len(edge_rows) else ';'}" for i,(c,p,o,d) in enumerate(edge_rows)]
        lines.append("")

    selector_rows = []
    for concept_id in sorted(desired_selectors):
        ordinal = max((o for system, _, o in selectors.get(concept_id, []) if system == "lcc"), default=-1) + 1
        for selector in sorted(desired_selectors[concept_id] - existing_selectors.get(concept_id, set())):
            selector_rows.append((concept_id, selector, ordinal))
            ordinal += 1
    if selector_rows:
        lines.append("INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES")
        lines += [f"    ({sql(c)},'lcc',{sql(s)},{o}){',' if i+1<len(selector_rows) else ';'}" for i,(c,s,o) in enumerate(selector_rows)]
        lines.append("")

    label_rows = []
    for concept_id in sorted(desired_labels):
        ordinal = max((o for _, o in labels.get(concept_id, [])), default=-1) + 1
        for label in sorted(desired_labels[concept_id] - existing_labels.get(concept_id, set())):
            label_rows.append((concept_id, label, ordinal))
            ordinal += 1
    if label_rows:
        lines.append("INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES")
        lines += [f"    ({sql(c)},{sql(label)},{o}){',' if i+1<len(label_rows) else ';'}" for i,(c,label,o) in enumerate(label_rows)]
        lines.append("")
    lines += ["COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--taxonomy", type=Path, default=Path("client/library-model/data/unified-taxonomy-v2.sqlite3"))
    parser.add_argument("--snapshot", type=Path, default=Path("client/library-model/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--manifest", type=Path, default=Path("client/library-model/data/remaining-lcc-one-level-manifest.json"))
    parser.add_argument("--output", type=Path, default=Path("client/library-model/data/migrations/20260817_remaining_lcc_one_level_expansion.sql"))
    parser.add_argument("--rebuild-manifest", action="store_true")
    args = parser.parse_args()
    if args.manifest.exists() and not args.rebuild_manifest:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    else:
        manifest = build_manifest(args.taxonomy, args.snapshot)
        args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    args.output.write_text(render_sql(manifest, args.taxonomy), encoding="utf-8")
    print(json.dumps({
        "frozen_lcc_leaves": manifest["frozen_lcc_leaf_count"],
        "handled_parents": len(manifest["handled_parents"]),
        "excluded_parents": len(manifest["excluded_parents"]),
        "unresolved_parents": len(manifest["unresolved_parents"]),
        "non_expandable_parents": len(manifest.get("non_expandable_parents", [])),
        "child_concepts": len(manifest["children"]),
    }, indent=2))


if __name__ == "__main__":
    main()
