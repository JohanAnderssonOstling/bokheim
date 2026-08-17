#!/usr/bin/env python3
"""Unify concepts duplicated by parallel imports of the same source record."""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
from collections import defaultdict
from pathlib import Path


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def normalized(value: str) -> str:
    return " ".join(re.sub(r"[^a-z0-9]+", " ", value.casefold()).split())


class UnionFind:
    def __init__(self, values: set[str]) -> None:
        self.parent = {value: value for value in values}

    def find(self, value: str) -> str:
        root = value
        while self.parent[root] != root:
            root = self.parent[root]
        while self.parent[value] != value:
            value, self.parent[value] = self.parent[value], root
        return root

    def union(self, left: str, right: str) -> bool:
        left, right = self.find(left), self.find(right)
        if left == right:
            return False
        self.parent[max(left, right)] = min(left, right)
        return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--taxonomy", type=Path,
        default=Path("client/library-model/data/unified-taxonomy-v2.sqlite3"),
    )
    parser.add_argument(
        "--manifest", type=Path,
        default=Path("client/library-model/data/unified-taxonomy-deduplication-manifest.json"),
    )
    parser.add_argument(
        "--output", type=Path,
        default=Path("client/library-model/data/migrations/20260817_unified_taxonomy_deduplication.sql"),
    )
    parser.add_argument(
        "--merge-parent-child-labels",
        action="store_true",
        help=(
            "also contract direct parent/child concepts whose preferred labels "
            "normalize identically; use this after structural imports to remove "
            "redundant wrapper concepts"
        ),
    )
    args = parser.parse_args()

    with sqlite3.connect(args.taxonomy) as db:
        concepts = dict(db.execute("SELECT concept_id,preferred_label FROM concept"))
        parent_rows = list(db.execute(
            "SELECT concept_id,parent_concept_id,ordinal,display_order FROM concept_parent"
        ))
        selector_rows = list(db.execute(
            "SELECT concept_id,system_id,selector,ordinal FROM source_selector"
        ))
        label_rows = list(db.execute(
            "SELECT concept_id,label,ordinal FROM source_label"
        ))

    uf = UnionFind(set(concepts))
    selector_groups: dict[tuple[str, str, str], list[str]] = defaultdict(list)
    for cid, system, selector, _ordinal in selector_rows:
        selector_groups[(system, selector, normalized(concepts[cid]))].append(cid)
    source_duplicate_groups = 0
    for values in selector_groups.values():
        unique = sorted(set(values))
        if len(unique) < 2:
            continue
        source_duplicate_groups += 1
        for cid in unique[1:]:
            uf.union(unique[0], cid)

    parent_child_label_groups = 0
    if args.merge_parent_child_labels:
        for child, parent, _ordinal, _display in parent_rows:
            if normalized(concepts[child]) != normalized(concepts[parent]):
                continue
            parent_child_label_groups += 1
            uf.union(child, parent)

    # Once duplicate parents are unified, their identically named children
    # become siblings. Merge those recursively to reconcile whole parallel
    # subtrees, even when jurisdiction-specific selectors differ.
    sibling_merge_rounds = 0
    while True:
        groups: dict[tuple[str, str], list[str]] = defaultdict(list)
        for child, parent, _ordinal, _display in parent_rows:
            child_root, parent_root = uf.find(child), uf.find(parent)
            if child_root == parent_root:
                continue
            groups[(parent_root, normalized(concepts[child_root]))].append(child_root)
        changed = False
        for values in groups.values():
            unique = sorted(set(values))
            for cid in unique[1:]:
                changed |= uf.union(unique[0], cid)
        if not changed:
            break
        sibling_merge_rounds += 1

    components: dict[str, set[str]] = defaultdict(set)
    for cid in concepts:
        components[uf.find(cid)].add(cid)
    duplicate_components = [values for values in components.values() if len(values) > 1]

    parents_by_child: dict[str, set[str]] = defaultdict(set)
    children_by_parent: dict[str, set[str]] = defaultdict(set)
    selectors_by_concept: dict[str, list[tuple[str, str, int]]] = defaultdict(list)
    labels_by_concept: dict[str, list[tuple[str, int]]] = defaultdict(list)
    for child, parent, _ordinal, _display in parent_rows:
        parents_by_child[child].add(parent)
        children_by_parent[parent].add(child)
    for cid, system, value, ordinal in selector_rows:
        selectors_by_concept[cid].append((system, value, ordinal))
    for cid, value, ordinal in label_rows:
        labels_by_concept[cid].append((value, ordinal))

    def canonical(values: set[str]) -> str:
        def score(cid: str) -> tuple[int, int, int, int, str]:
            label = concepts[cid]
            return (
                0 if cid.startswith("unified:") else 1,
                0 if not label.rstrip().endswith(".") else 1,
                -len(parents_by_child[cid]),
                -len(children_by_parent[cid]),
                cid,
            )
        return min(values, key=score)

    representative: dict[str, str] = {cid: cid for cid in concepts}
    component_rows = []
    for values in duplicate_components:
        winner = canonical(values)
        for cid in values:
            representative[cid] = winner
        component_rows.append((winner, sorted(values - {winner})))
    losers = {loser for _winner, values in component_rows for loser in values}

    # Contract the graph and verify that unification cannot create a cycle.
    edge_display: dict[tuple[str, str], int] = {}
    for child, parent, _ordinal, display in parent_rows:
        child, parent = representative[child], representative[parent]
        if child == parent:
            continue
        edge_display[(child, parent)] = min(edge_display.get((child, parent), display), display)
    adjacency: dict[str, set[str]] = defaultdict(set)
    for child, parent in edge_display:
        adjacency[child].add(parent)
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(cid: str) -> None:
        if cid in visiting:
            raise RuntimeError(f"deduplication would create a cycle at {cid}")
        if cid in visited:
            return
        visiting.add(cid)
        for parent in adjacency.get(cid, set()):
            visit(parent)
        visiting.remove(cid)
        visited.add(cid)

    for cid in concepts:
        if cid not in losers:
            visit(cid)

    # Contraction must leave at most one normalized label per sibling group.
    sibling_groups: dict[tuple[str, str], set[str]] = defaultdict(set)
    for child, parent in edge_display:
        sibling_groups[(parent, normalized(concepts[child]))].add(child)
    collisions = [values for values in sibling_groups.values() if len(values) > 1]
    if collisions:
        raise RuntimeError(f"deduplication leaves {len(collisions)} duplicate sibling groups")

    existing_edges = {
        (child, parent) for child, parent, _ordinal, _display in parent_rows
        if child not in losers and parent not in losers
    }
    next_parent_ordinal = defaultdict(int)
    for child, _parent, ordinal, _display in parent_rows:
        if child not in losers:
            next_parent_ordinal[child] = max(next_parent_ordinal[child], ordinal + 1)
    added_edges: list[tuple[str, str, int, int]] = []
    for child, parent in sorted(edge_display):
        if (child, parent) in existing_edges:
            continue
        ordinal = next_parent_ordinal[child]
        next_parent_ordinal[child] += 1
        added_edges.append((child, parent, ordinal, edge_display[(child, parent)]))

    existing_selectors = {
        cid: {(system, value) for system, value, _ordinal in values}
        for cid, values in selectors_by_concept.items()
    }
    next_selector = defaultdict(lambda: defaultdict(int))
    for cid, values in selectors_by_concept.items():
        for system, _value, ordinal in values:
            next_selector[cid][system] = max(next_selector[cid][system], ordinal + 1)
    existing_labels = {
        cid: {value for value, _ordinal in values}
        for cid, values in labels_by_concept.items()
    }
    next_label = defaultdict(int)
    for cid, values in labels_by_concept.items():
        next_label[cid] = max((ordinal for _value, ordinal in values), default=-1) + 1

    added_selectors: list[tuple[str, str, str, int]] = []
    added_labels: list[tuple[str, str, int]] = []
    renames: dict[str, str] = {}
    for winner, component_losers in sorted(component_rows):
        preferred_candidates = [concepts[winner], *(concepts[cid] for cid in component_losers)]
        clean = [value.rstrip().rstrip(".") for value in preferred_candidates]
        preferred = min(clean, key=lambda value: (len(value), value.casefold()))
        if concepts[winner] != preferred:
            renames[winner] = preferred
        for loser in component_losers:
            for system, value, _ordinal in selectors_by_concept.get(loser, []):
                if (system, value) in existing_selectors.setdefault(winner, set()):
                    continue
                ordinal = next_selector[winner][system]
                next_selector[winner][system] += 1
                existing_selectors[winner].add((system, value))
                added_selectors.append((winner, system, value, ordinal))
            for value in [concepts[loser], *(label for label, _ in labels_by_concept.get(loser, []))]:
                if value in existing_labels.setdefault(winner, set()):
                    continue
                ordinal = next_label[winner]
                next_label[winner] += 1
                existing_labels[winner].add(value)
                added_labels.append((winner, value, ordinal))

    purpose = (
        "Unify duplicate source records and redundant same-label parent/child wrappers."
        if args.merge_parent_child_labels
        else "Unify concepts duplicated by parallel imports of identical source records."
    )
    lines = [
        f"-- {purpose}",
        "-- Generated by scripts/generate-unified-taxonomy-deduplication.py.",
        "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
    ]
    for cid, value in sorted(renames.items()):
        lines.append(f"UPDATE concept SET preferred_label={sql(value)} WHERE concept_id={sql(cid)};")
    if renames:
        lines.append("")
    if added_selectors:
        lines.append("INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES")
        lines += [
            f"    ({sql(cid)},{sql(system)},{sql(value)},{ordinal})"
            f"{',' if index + 1 < len(added_selectors) else ';'}"
            for index, (cid, system, value, ordinal) in enumerate(added_selectors)
        ]
        lines.append("")
    if added_labels:
        lines.append("INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES")
        lines += [
            f"    ({sql(cid)},{sql(value)},{ordinal})"
            f"{',' if index + 1 < len(added_labels) else ';'}"
            for index, (cid, value, ordinal) in enumerate(added_labels)
        ]
        lines.append("")
    lines.append("DELETE FROM concept_parent WHERE concept_id IN (")
    lines += [
        f"    {sql(cid)}{',' if index + 1 < len(losers) else ''}"
        for index, cid in enumerate(sorted(losers))
    ]
    lines.append(") OR parent_concept_id IN (")
    lines += [
        f"    {sql(cid)}{',' if index + 1 < len(losers) else ''}"
        for index, cid in enumerate(sorted(losers))
    ]
    lines.append(");")
    if added_edges:
        lines.append("INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES")
        lines += [
            f"    ({sql(child)},{sql(parent)},{ordinal},{display})"
            f"{',' if index + 1 < len(added_edges) else ';'}"
            for index, (child, parent, ordinal, display) in enumerate(added_edges)
        ]
    lines.append("DELETE FROM concept WHERE concept_id IN (")
    lines += [
        f"    {sql(cid)}{',' if index + 1 < len(losers) else ''}"
        for index, cid in enumerate(sorted(losers))
    ]
    lines += [");", "", "COMMIT;", ""]

    manifest = {
        "version": 1,
        "source_duplicate_groups": source_duplicate_groups,
        "parent_child_label_groups": parent_child_label_groups,
        "sibling_merge_rounds": sibling_merge_rounds,
        "components": [
            {"winner_id": winner, "loser_ids": values}
            for winner, values in sorted(component_rows)
        ],
        "removed_concepts": len(losers),
        "added_edges": len(added_edges),
        "transferred_selectors": len(added_selectors),
        "transferred_labels": len(added_labels),
        "renamed_concepts": len(renames),
    }
    args.manifest.write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    args.output.write_text("\n".join(lines), encoding="utf-8")
    print(json.dumps({key: value for key, value in manifest.items() if key != "components"}, indent=2))


if __name__ == "__main__":
    main()
