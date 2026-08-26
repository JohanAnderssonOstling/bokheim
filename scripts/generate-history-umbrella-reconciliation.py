#!/usr/bin/env python3
"""Remove redundant `place / History` wrappers in regional History."""

from __future__ import annotations

import argparse
import json
import sqlite3
from collections import defaultdict
from pathlib import Path


def normalized(value: str) -> str:
    return " ".join(value.casefold().split())


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def build_manifest(path: Path) -> dict[str, object]:
    with sqlite3.connect(path) as db:
        concepts = dict(db.execute("SELECT concept_id,preferred_label FROM concept"))
        parent_rows = list(db.execute("SELECT concept_id,parent_concept_id,ordinal,display_order FROM concept_parent"))
        selector_rows = list(db.execute("SELECT concept_id,system_id,selector,ordinal FROM source_selector"))
        label_rows = list(db.execute("SELECT concept_id,label,ordinal FROM source_label"))
    parents: dict[str, list[str]] = defaultdict(list)
    children: dict[str, list[str]] = defaultdict(list)
    edge_meta = {}
    for child, parent, ordinal, order in parent_rows:
        parents[child].append(parent); children[parent].append(child)
        edge_meta[(child, parent)] = (ordinal, order)
    root = next(cid for cid, label in concepts.items() if label == "History by Region")
    regional = {root}
    queue = [root]
    while queue:
        parent = queue.pop()
        for child in children.get(parent, []):
            if child not in regional:
                regional.add(child); queue.append(child)

    selector_map: dict[str, list[tuple[str, str, int]]] = defaultdict(list)
    source_label_map: dict[str, list[tuple[str, int]]] = defaultdict(list)
    for cid, system, selector, ordinal in selector_rows: selector_map[cid].append((system, selector, ordinal))
    for cid, label, ordinal in label_rows: source_label_map[cid].append((label, ordinal))

    transfers: list[tuple[str, str]] = []
    moves: list[tuple[str, str, str, int, int]] = []
    deleted: set[str] = set()

    def direct_match(parent: str, label: str, except_id: str) -> str | None:
        matches = [
            cid for cid in children.get(parent, [])
            if cid != except_id and cid not in deleted and normalized(concepts[cid]) == normalized(label)
        ]
        return matches[0] if len(matches) == 1 else None

    def merge(source: str, target: str) -> None:
        if source in deleted or source == target:
            return
        transfers.append((source, target))
        for child in list(children.get(source, [])):
            match = direct_match(target, concepts[child], child)
            if match is not None and len(parents[child]) == 1:
                merge(child, match)
                continue
            ordinal, order = edge_meta[(child, source)]
            moves.append((child, source, target, ordinal, order))
            children[source].remove(child)
            parents[child].remove(source)
            children[target].append(child)
            parents[child].append(target)
            edge_meta[(child, target)] = (ordinal, order)
        deleted.add(source)
        for parent in list(parents[source]):
            if source in children[parent]: children[parent].remove(source)

    umbrellas = sorted(
        cid for cid in regional
        if cid != root and normalized(concepts[cid]) == "history" and len(parents[cid]) == 1
    )
    for umbrella in umbrellas:
        if umbrella not in deleted:
            merge(umbrella, parents[umbrella][0])

    return {
        "version": 1,
        "umbrella_count": len(umbrellas),
        "transfers": [{"source_id": source, "target_id": target} for source, target in transfers],
        "moves": [
            {"child_id": child, "old_parent_id": old, "new_parent_id": new,
             "ordinal": ordinal, "display_order": order}
            for child, old, new, ordinal, order in moves
        ],
        "delete_concepts": sorted(deleted),
        "selectors": {
            cid: [{"system": system, "selector": selector} for system, selector, _ in selector_map.get(cid, [])]
            for cid in deleted
        },
        "labels": {
            cid: [label for label, _ in source_label_map.get(cid, [])]
            for cid in deleted
        },
    }


def render(manifest: dict[str, object], taxonomy: Path) -> str:
    with sqlite3.connect(taxonomy) as db:
        selectors = list(db.execute("SELECT concept_id,system_id,selector,ordinal FROM source_selector"))
        labels = list(db.execute("SELECT concept_id,label,ordinal FROM source_label"))
    existing_selectors: dict[str, set[tuple[str, str]]] = defaultdict(set)
    existing_labels: dict[str, set[str]] = defaultdict(set)
    next_selector: dict[tuple[str, str], int] = defaultdict(int)
    next_label: dict[str, int] = defaultdict(int)
    for cid, system, value, ordinal in selectors:
        existing_selectors[cid].add((system, value)); next_selector[(cid, system)] = max(next_selector[(cid, system)], ordinal + 1)
    for cid, value, ordinal in labels:
        existing_labels[cid].add(value); next_label[cid] = max(next_label[cid], ordinal + 1)
    selector_rows = []
    label_rows_out = []
    for transfer in manifest["transfers"]:
        source, target = transfer["source_id"], transfer["target_id"]
        for row in manifest["selectors"].get(source, []):
            key = (row["system"], row["selector"])
            if key in existing_selectors[target]: continue
            ordinal = next_selector[(target, row["system"])]; next_selector[(target, row["system"])] += 1
            existing_selectors[target].add(key)
            selector_rows.append((target, row["system"], row["selector"], ordinal))
        for value in manifest["labels"].get(source, []):
            if value in existing_labels[target]: continue
            ordinal = next_label[target]; next_label[target] += 1
            existing_labels[target].add(value)
            label_rows_out.append((target, value, ordinal))
    lines = [
        "-- Remove redundant regional-History `place / History` wrappers.",
        "-- Selectors are retained on the place; children are lifted or merged by label.",
        "-- Generated by scripts/generate-history-umbrella-reconciliation.py.",
        "", "PRAGMA foreign_keys = ON;", "BEGIN IMMEDIATE;", "",
    ]
    if selector_rows:
        lines.append("INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES")
        lines += [f"    ({sql(c)},{sql(s)},{sql(v)},{o}){',' if i + 1 < len(selector_rows) else ';'}" for i, (c, s, v, o) in enumerate(selector_rows)]
        lines.append("")
    if label_rows_out:
        lines.append("INSERT OR IGNORE INTO source_label(concept_id,label,ordinal) VALUES")
        lines += [f"    ({sql(c)},{sql(v)},{o}){',' if i + 1 < len(label_rows_out) else ';'}" for i, (c, v, o) in enumerate(label_rows_out)]
        lines.append("")
    for move in manifest["moves"]:
        lines.append(
            f"UPDATE concept_parent SET parent_concept_id={sql(move['new_parent_id'])} "
            f"WHERE concept_id={sql(move['child_id'])} AND parent_concept_id={sql(move['old_parent_id'])};"
        )
    if manifest["moves"]: lines.append("")
    deleted = manifest["delete_concepts"]
    if deleted:
        ids = ",".join(sql(cid) for cid in deleted)
        lines += [
            f"DELETE FROM concept_parent WHERE concept_id IN ({ids}) OR parent_concept_id IN ({ids});",
            f"DELETE FROM source_selector WHERE concept_id IN ({ids});",
            f"DELETE FROM source_label WHERE concept_id IN ({ids});",
            f"DELETE FROM concept WHERE concept_id IN ({ids});", "",
        ]
    lines += ["COMMIT;", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--taxonomy", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, default=Path("shared/subject-projection/data/history-umbrella-reconciliation-manifest.json"))
    parser.add_argument("--output", type=Path, default=Path("shared/subject-projection/data/migrations/20260817_history_umbrella_reconciliation.sql"))
    args = parser.parse_args()
    manifest = build_manifest(args.taxonomy)
    args.manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    args.output.write_text(render(manifest, args.taxonomy), encoding="utf-8")
    print(json.dumps({"umbrellas": manifest["umbrella_count"], "merged_concepts": len(manifest["delete_concepts"]), "moved_edges": len(manifest["moves"])}, indent=2))


if __name__ == "__main__":
    main()
