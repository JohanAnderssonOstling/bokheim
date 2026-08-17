#!/usr/bin/env python3
"""Re-anchor regional History schedules and add one semantic LCC level.

The outline-derived taxonomy occasionally collapsed several national schedules
into a regional leaf (for example Northern Europe / By period).  This generator
uses the structural MDSConnect paths to put each range beneath its country (or
the genuinely shared region) before exposing one further semantic level.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sqlite3
import sys
import uuid
from collections import defaultdict
from pathlib import Path


HERE = Path(__file__).resolve().parent
CORE_PATH = HERE / "generate-remaining-lcc-expansions.py"
SPEC = importlib.util.spec_from_file_location("lcc_expansion_core", CORE_PATH)
assert SPEC and SPEC.loader
core = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = core
SPEC.loader.exec_module(core)

NAMESPACE = uuid.UUID("66ddb92c-0406-5b93-bf34-74dfd9d7e86a")
HISTORY_ROUTE = ("humanities", "history", "history by region")
MACRO_REGIONS = {"africa", "americas", "asia", "europe", "oceania"}
GENERIC_PATH_LABELS = {"history", "general history"}
PERSON_DATE_RE = re.compile(r"^([^,;:]{2,80}),\s*(?:ca\.?\s*)?\d{3,4}(?:\s*[-–/]\s*\d{0,4})?", re.I)
FLIPPED_PERSON_RE = re.compile(r"^([^,]{2,60}),\s*([A-ZÀ-ÖØ-Þ][^,]{1,60}),\s*(?:ca\.?\s*)?\d{3,4}", re.I)
REIGN_WITH_NAME_RE = re.compile(r"^\d{3,4}[^.]*\.\s*[^,;:]*\b[IVXLCDM]{1,8}$")
PERSON_CONTEXT_RE = re.compile(
    r"(?:^|\b)(?:biography of|individual rulers?|individual persons?|personal names?)(?:\b|$)", re.I
)


def history_routes(routes: list[tuple[str, ...]]) -> list[tuple[str, ...]]:
    result = []
    for route in routes:
        normalized = tuple(core.normalize_label(label) for label in route)
        if normalized[:3] == HISTORY_ROUTE:
            result.append(route)
    return result


def aliases(label: str) -> set[str]:
    normalized = core.normalize_label(label)
    values = {normalized}
    for prefix in ("history of ", "history of the "):
        if normalized.startswith(prefix):
            values.add(normalized[len(prefix):])
    values.add(normalized.removesuffix(" history"))
    return {value for value in values if value}


def semantic_exclusion(label: str) -> str | None:
    normalized = core.normalize_label(label)
    if core.FILING_RE.search(label) or core.ALPHA_BUCKET_RE.fullmatch(label.strip()):
        return "alphabetic filing record"
    if core.is_form_caption(label):
        return "bibliographic or form subdivision"
    if core.is_geographic_container_caption(label):
        return "geographic/locality authority subdivision"
    if normalized.startswith("description and travel"):
        return "description-and-travel route"
    dated = PERSON_DATE_RE.match(label.strip())
    dated_person = False
    if dated:
        prefix = dated.group(1).strip()
        prefix_words = set(core.normalize_label(prefix).split())
        era_terms = {"war", "period", "century", "dynasty", "empire", "republic", "revolution"}
        dated_person = bool(
            (re.search(r"\b[IVXLCDM]{1,8}\b", prefix) and not prefix_words & era_terms)
            or re.match(r"^(?:king|queen|emperor|empress|pope|tsar|czar)\b", prefix, re.I)
        )
    flipped = FLIPPED_PERSON_RE.match(label.strip())
    flipped_person = bool(
        flipped
        and len(core.normalize_label(flipped.group(1)).split()) == 1
        and len(core.normalize_label(flipped.group(2)).split()) == 1
        and not re.search(r"\d", flipped.group(2))
    )
    if flipped_person or REIGN_WITH_NAME_RE.match(label.strip()) or dated_person:
        return "named-person authority record"
    if PERSON_CONTEXT_RE.search(normalized):
        return "named-person authority record"
    return None


def common_prefix(left: tuple[str, ...], right: tuple[str, ...]) -> int:
    count = 0
    for a, b in zip(left, right):
        if core.normalize_label(a) != core.normalize_label(b):
            break
        count += 1
    return count


def build_manifest(taxonomy_path: Path, snapshot_path: Path) -> dict[str, object]:
    taxonomy = core.load_taxonomy(taxonomy_path)
    records, child_index, by_label, by_span, by_class1, sorted_labels = core.load_snapshot(snapshot_path)
    concepts: dict[str, str] = taxonomy["concepts"]
    parents: dict[str, list[tuple[str, int, int]]] = taxonomy["parents"]
    children: dict[str, list[str]] = taxonomy["children"]
    selectors: dict[str, list[tuple[str, str, int]]] = taxonomy["selectors"]
    routes: dict[str, list[tuple[str, ...]]] = taxonomy["routes"]

    regional_ids = {cid for cid in concepts if history_routes(routes[cid])}
    macro_by_id: dict[str, str] = {}
    anchor_index: dict[tuple[str, str], list[str]] = defaultdict(list)
    for cid in regional_ids:
        route = history_routes(routes[cid])[0]
        normalized = [core.normalize_label(label) for label in route]
        macro = next((label for label in normalized[3:] if label in MACRO_REGIONS), "")
        macro_by_id[cid] = macro
        for alias in aliases(concepts[cid]):
            anchor_index[(macro, alias)].append(cid)

    # This virtual child index includes nodes planned earlier in the run, so
    # repeated schedule paths converge on one contextual concept.
    direct: dict[tuple[str, str], list[str]] = defaultdict(list)
    for child_id, values in parents.items():
        for parent_id, _, _ in values:
            direct[(parent_id, core.normalize_label(concepts[child_id]))].append(child_id)

    new_nodes: dict[str, str] = {}
    edges: set[tuple[str, str]] = set()
    desired_selectors: dict[str, set[str]] = defaultdict(set)
    desired_labels: dict[str, set[str]] = defaultdict(set)
    source_targets: dict[str, set[str]] = defaultdict(set)
    source_records: dict[str, set[int]] = defaultdict(set)
    excluded_counts: dict[str, int] = defaultdict(int)
    excluded_examples: dict[str, list[str]] = defaultdict(list)
    handled: list[dict[str, object]] = []
    excluded_parents: list[dict[str, str]] = []

    def label_of(cid: str) -> str:
        return new_nodes.get(cid, concepts.get(cid, ""))

    def ensure_node(parent_id: str, label: str, seed: str) -> str:
        key = (parent_id, core.normalize_label(label))
        existing = direct.get(key, [])
        if len(existing) == 1:
            return existing[0]
        cid = "bokheim:history-lcc:" + str(uuid.uuid5(NAMESPACE, f"{parent_id}\n{core.normalize_label(label)}\n{seed}"))
        if cid not in concepts:
            new_nodes[cid] = label
        edges.add((cid, parent_id))
        if cid not in direct[key]:
            direct[key].append(cid)
        return cid

    def select_anchor(source_id: str, record: core.Record) -> tuple[str, int] | None:
        macro = macro_by_id[source_id]
        full = record.full_path
        normalized = [core.normalize_label(label) for label in full]
        history_at = next((i for i, label in enumerate(normalized) if label == "history"), len(full))
        # Local-history/city filing schedules are deliberately not promoted.
        if any("local history and description" in label for label in normalized[:history_at]):
            return None
        source_route = history_routes(routes[source_id])[0]
        candidates: list[tuple[int, int, int, str, int]] = []
        for index, component in enumerate(full[:history_at]):
            for alias in aliases(component):
                for cid in anchor_index.get((macro, alias), []):
                    route = history_routes(routes[cid])[0]
                    candidates.append((index, common_prefix(source_route, route), len(route), cid, index))
        if not candidates:
            return None
        candidates.sort(reverse=True)
        _, _, _, cid, index = candidates[0]
        return cid, index

    frozen_leaves = sorted(
        cid for cid in regional_ids
        if not children.get(cid)
        and any(system == "lcc" for system, _, _ in selectors.get(cid, []))
        and core.normalize_label(concepts[cid]) not in {"description and travel", "local history and description"}
    )

    for source_id in frozen_leaves:
        parent_records, error = core.choose_parent_records(
            source_id, taxonomy, by_label, by_span, by_class1, sorted_labels
        )
        if error:
            excluded_parents.append({"concept_id": source_id, "label": concepts[source_id], "reason": error})
            continue
        placements = 0
        for parent_record in parent_records:
            anchor = select_anchor(source_id, parent_record)
            if anchor is None:
                excluded_parents.append({
                    "concept_id": source_id, "label": concepts[source_id],
                    "reason": f"no safe country/region anchor for record {parent_record.record_id}",
                })
                continue
            anchor_id, anchor_index_in_path = anchor
            tail = [
                label for label in parent_record.full_path[anchor_index_in_path + 1:]
                if core.normalize_label(label) not in GENERIC_PATH_LABELS
            ]
            if any(semantic_exclusion(label) for label in tail):
                reason = next(semantic_exclusion(label) for label in tail if semantic_exclusion(label))
                excluded_counts[str(reason)] += 1
                if len(excluded_examples[str(reason)]) < 20:
                    excluded_examples[str(reason)].append(" / ".join(tail))
                continue

            parent_matches_source = core.normalize_label(parent_record.caption) == core.normalize_label(concepts[source_id])
            if parent_matches_source:
                structural_tail = tail
                additions = child_index.get(parent_record.full_path, [])
            else:
                structural_tail = tail[:-1]
                additions = [parent_record]

            context_id = anchor_id
            for depth, label in enumerate(structural_tail):
                # Reuse the source concept only when its existing route already
                # expresses this same anchor and complete structural tail.
                source_route_norm = [core.normalize_label(x) for x in history_routes(routes[source_id])[0]]
                desired_norm = core.normalize_label(label)
                if (
                    depth == len(structural_tail) - 1
                    and desired_norm == core.normalize_label(concepts[source_id])
                    and core.normalize_label(concepts[anchor_id]) in source_route_norm
                ):
                    context_id = source_id
                else:
                    context_id = ensure_node(context_id, label, str(parent_record.record_id))
            desired_selectors[context_id].add(core.selector_for(parent_record))
            desired_labels[context_id].add(parent_record.caption)
            source_targets[source_id].add(context_id)
            source_records[source_id].add(parent_record.record_id)

            for addition in additions:
                reason = semantic_exclusion(addition.caption)
                if reason:
                    excluded_counts[reason] += 1
                    if len(excluded_examples[reason]) < 20:
                        excluded_examples[reason].append(addition.caption)
                    continue
                target = ensure_node(context_id, addition.caption, str(addition.record_id))
                parent_priority = max(
                    (core.selector_priority(value) for system, value, _ in selectors.get(source_id, []) if system == "lcc"),
                    default=4,
                )
                desired_selectors[target].add(f"{addition.start}..{addition.end}@{parent_priority + 1}")
                desired_labels[target].add(addition.caption)
                placements += 1
        if placements:
            handled.append({
                "concept_id": source_id, "label": concepts[source_id],
                "snapshot_parent_record_ids": sorted(source_records[source_id]),
                "placement_count": placements,
            })

    # Delete only empty, LCC-only leaves whose structural records were wholly
    # moved to other contextual concepts. Broader multi-system concepts remain.
    delete_sources = []
    protected_contexts = {target for targets in source_targets.values() for target in targets}
    protected_contexts.update(parent for _, parent in edges)
    for source_id, targets in source_targets.items():
        source_systems = {system for system, _, _ in selectors.get(source_id, [])}
        if (
            source_id not in targets
            and source_id not in protected_contexts
            and source_systems == {"lcc"}
            and not children.get(source_id)
            and len(source_records[source_id]) == len(core.choose_parent_records(
                source_id, taxonomy, by_label, by_span, by_class1, sorted_labels
            )[0])
        ):
            delete_sources.append(source_id)

    return {
        "version": 1,
        "frozen_history_leaf_count": len(frozen_leaves),
        "handled_parents": handled,
        "excluded_parents": excluded_parents,
        "excluded_records": {
            reason: {"count": count, "examples": excluded_examples[reason]}
            for reason, count in sorted(excluded_counts.items())
        },
        "new_nodes": [{"concept_id": cid, "preferred_label": label} for cid, label in sorted(new_nodes.items())],
        "edges": [{"child_id": child, "parent_id": parent} for child, parent in sorted(edges)],
        "selectors": [
            {"concept_id": cid, "selector": selector}
            for cid in sorted(desired_selectors) for selector in sorted(desired_selectors[cid])
        ],
        "labels": [
            {"concept_id": cid, "label": label}
            for cid in sorted(desired_labels) for label in sorted(desired_labels[cid])
        ],
        "delete_sources": sorted(delete_sources),
    }


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def render_sql(manifest: dict[str, object], taxonomy_path: Path) -> str:
    taxonomy = core.load_taxonomy(taxonomy_path)
    parents = taxonomy["parents"]
    selectors = taxonomy["selectors"]
    labels = taxonomy["labels"]
    existing_edges = {(child, parent) for child, values in parents.items() for parent, _, _ in values}
    existing_selectors = {cid: {value for system, value, _ in values if system == "lcc"} for cid, values in selectors.items()}
    existing_labels = {cid: {value for value, _ in values} for cid, values in labels.items()}
    lines = [
        "-- Country-aware regional History restructuring and one further semantic LCC level.",
        "-- Named people, forms, localities, and filing records remain excluded.",
        "-- Source: Library of Congress MDSConnect Classification 2016 retrospective.",
        "-- Generated by scripts/generate-history-lcc-expansion.py.",
        "-- Apply once to the frozen pre-continuation taxonomy, before the umbrella reconciliation.",
        "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
    ]
    nodes = manifest["new_nodes"]
    if nodes:
        lines.append("INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES")
        lines += [f"    ({sql(row['concept_id'])},{sql(row['preferred_label'])}){',' if i + 1 < len(nodes) else ';'}" for i, row in enumerate(nodes)]
        lines.append("")
    new_edges = [(row["child_id"], row["parent_id"]) for row in manifest["edges"] if (row["child_id"], row["parent_id"]) not in existing_edges]
    if new_edges:
        next_ordinal = {cid: max((ordinal for _, ordinal, _ in values), default=-1) + 1 for cid, values in parents.items()}
        order = defaultdict(int)
        rows = []
        for child, parent in new_edges:
            ordinal = next_ordinal.get(child, 0); next_ordinal[child] = ordinal + 1
            order[parent] += 10
            rows.append((child, parent, ordinal, order[parent]))
        lines.append("INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES")
        lines += [f"    ({sql(c)},{sql(p)},{o},{d}){',' if i + 1 < len(rows) else ';'}" for i, (c, p, o, d) in enumerate(rows)]
        lines.append("")
    selector_rows = []
    next_selector = {cid: max((o for system, _, o in values if system == "lcc"), default=-1) + 1 for cid, values in selectors.items()}
    for row in manifest["selectors"]:
        cid, value = row["concept_id"], row["selector"]
        if value in existing_selectors.get(cid, set()): continue
        ordinal = next_selector.get(cid, 0); next_selector[cid] = ordinal + 1
        selector_rows.append((cid, value, ordinal))
    if selector_rows:
        lines.append("INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES")
        lines += [f"    ({sql(c)},'lcc',{sql(v)},{o}){',' if i + 1 < len(selector_rows) else ';'}" for i, (c, v, o) in enumerate(selector_rows)]
        lines.append("")
    label_rows = []
    next_label = {cid: max((o for _, o in values), default=-1) + 1 for cid, values in labels.items()}
    for row in manifest["labels"]:
        cid, value = row["concept_id"], row["label"]
        if value in existing_labels.get(cid, set()): continue
        ordinal = next_label.get(cid, 0); next_label[cid] = ordinal + 1
        label_rows.append((cid, value, ordinal))
    if label_rows:
        lines.append("INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES")
        lines += [f"    ({sql(c)},{sql(v)},{o}){',' if i + 1 < len(label_rows) else ';'}" for i, (c, v, o) in enumerate(label_rows)]
        lines.append("")
    deleted = manifest["delete_sources"]
    if deleted:
        ids = ",".join(sql(cid) for cid in deleted)
        lines += [
            "-- Remove obsolete collapsed LCC-only leaves after their ranges have been re-anchored.",
            f"DELETE FROM concept_parent WHERE concept_id IN ({ids}) OR parent_concept_id IN ({ids});",
            f"DELETE FROM source_selector WHERE concept_id IN ({ids});",
            f"DELETE FROM source_label WHERE concept_id IN ({ids});",
            f"DELETE FROM concept WHERE concept_id IN ({ids});", "",
        ]
    lines += ["COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--taxonomy", type=Path, default=Path("client/library-model/data/unified-taxonomy-v2.sqlite3"))
    parser.add_argument("--snapshot", type=Path, default=Path("client/library-model/data/lcc-mdsconnect-2016.sqlite3"))
    parser.add_argument("--manifest", type=Path, default=Path("client/library-model/data/history-lcc-next-level-manifest.json"))
    parser.add_argument("--output", type=Path, default=Path("client/library-model/data/migrations/20260817_history_lcc_next_level.sql"))
    args = parser.parse_args()
    manifest = build_manifest(args.taxonomy, args.snapshot)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    args.output.write_text(render_sql(manifest, args.taxonomy), encoding="utf-8")
    print(json.dumps({
        "frozen_history_leaves": manifest["frozen_history_leaf_count"],
        "handled_parents": len(manifest["handled_parents"]),
        "new_nodes": len(manifest["new_nodes"]),
        "edges": len(manifest["edges"]),
        "selectors": len(manifest["selectors"]),
        "deleted_collapsed_sources": len(manifest["delete_sources"]),
    }, indent=2))


if __name__ == "__main__":
    main()
