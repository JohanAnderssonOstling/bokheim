#!/usr/bin/env python3
"""Reconcile structural duplicates introduced by the History locality import."""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
from collections import defaultdict
from pathlib import Path


STRUCTURAL_LABELS = {
    "Cities & Towns",
    "Local History & Places",
    "Regions & Administrative Divisions",
}
STATE_MERGES = {
    "bokheim:history-lcc-full:32baa0b3-9b85-51c1-a51e-1ce5e7b37cf2":
        "unified:27de458b-5965-5eed-8605-979a51bfd9fc",  # Alaska
    "bokheim:history-lcc-full:7d00e312-814f-50db-8436-bb736535e4b5":
        "unified:c88f2aa1-6718-5632-8f23-7b488f7c9e16",  # Utah
}
CORFU_MERGE = {
    "bokheim:history-lcc-full:6a2e2413-bd55-5e12-bfda-e41b21b23cac":
        "bokheim:history-lcc-full:34745cca-afde-5314-ae13-d1b1eadb0873",
}
AMBIGUOUS_SPAN_CONCEPTS = {
    "bokheim:history-lcc-full:0b3a232e-af66-5a55-ad9f-bf5d1b73a416",  # Chester
    "bokheim:history-lcc-full:13b705db-dd94-5d81-91b0-23384ae7712f",  # Chichester
}
SAINT_BUCKET_RE = re.compile(r"^saint(?:e)?\s+[a-z](?:\s*-?\s*saint(?:e)?\s+[a-z])?$", re.I)
PREFIX_BUCKET_RE = re.compile(
    r"^(?:la|le|les)\s*[a-z]*\s*-\s*(?:(?:la|le|les)\s*)?[a-z]+$", re.I
)


def sql(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def normalized(value: str) -> str:
    return " ".join(re.sub(r"[^a-z0-9]+", " ", value.casefold()).split())


def is_filing_bucket(label: str) -> bool:
    return bool(SAINT_BUCKET_RE.fullmatch(label.strip()) or PREFIX_BUCKET_RE.fullmatch(label.strip()))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--taxonomy", type=Path,
        default=Path("shared/subject-projection/data/unified-taxonomy-v2.sqlite3"),
    )
    parser.add_argument(
        "--locality-manifest", type=Path,
        default=Path("shared/subject-projection/data/history-lcc-localities-manifest.json"),
    )
    parser.add_argument(
        "--manifest", type=Path,
        default=Path("shared/subject-projection/data/history-locality-reconciliation-manifest.json"),
    )
    parser.add_argument(
        "--output", type=Path,
        default=Path("shared/subject-projection/data/migrations/20260817_history_locality_reconciliation.sql"),
    )
    args = parser.parse_args()

    locality_manifest = json.loads(args.locality_manifest.read_text(encoding="utf-8"))
    locality_ids = {row["concept_id"] for row in locality_manifest["new_nodes"]}
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

    parents: dict[str, list[str]] = defaultdict(list)
    children: dict[str, list[str]] = defaultdict(list)
    for child, parent, _ordinal, _display in parent_rows:
        parents[child].append(parent)
        children[parent].append(child)

    merges: dict[str, str] = {}
    reasons: dict[str, str] = {}

    # Collapse identical structural wrappers generated from successive LCC
    # captions such as "Cities, towns" and "Other cities, towns, A-Z".
    for child, parent, _ordinal, _display in parent_rows:
        if (
            concepts.get(child) in STRUCTURAL_LABELS
            and concepts.get(child) == concepts.get(parent)
            and child in locality_ids
        ):
            merges[child] = parent
            reasons[child] = "redundant nested structural wrapper"

    # Collapse French alphabetic guide buckets while retaining their named
    # descendants beneath the real geographic parent.
    for concept_id in sorted(locality_ids):
        label = concepts.get(concept_id, "")
        if (is_filing_bucket(label) or normalized(label) == "local") and len(parents[concept_id]) == 1:
            merges[concept_id] = parents[concept_id][0]
            reasons[concept_id] = "alphabetic or generic locality filing bucket"

    # Reuse pre-existing locality umbrellas that differ only in presentation.
    renames: dict[str, str] = {}
    by_parent: dict[str, list[str]] = defaultdict(list)
    for child, parent, _ordinal, _display in parent_rows:
        by_parent[parent].append(child)
    for siblings in by_parent.values():
        old = [cid for cid in siblings if concepts.get(cid) == "Local history and description"]
        new = [cid for cid in siblings if concepts.get(cid) == "Local History & Places"]
        if len(old) == 1 and len(new) == 1 and new[0] in locality_ids:
            merges[new[0]] = old[0]
            reasons[new[0]] = "parallel locality umbrella"
            renames[old[0]] = "Local History & Places"

    for loser, winner in STATE_MERGES.items():
        if loser in concepts and winner in concepts:
            merges[loser] = winner
            reasons[loser] = "duplicate state branch; reuse existing regional state concept"
    for loser, winner in CORFU_MERGE.items():
        if loser in concepts and winner in concepts:
            merges[loser] = winner
            reasons[loser] = "equivalent place labels with the same LCC span"
            renames[winner] = "Corfu (Kerkyra)"

    deletes = sorted(cid for cid in AMBIGUOUS_SPAN_CONCEPTS if cid in concepts)
    for concept_id in deletes:
        if children.get(concept_id):
            raise RuntimeError(f"ambiguous-span concept unexpectedly has children: {concept_id}")

    def winner(concept_id: str) -> str:
        seen = set()
        while concept_id in merges:
            if concept_id in seen:
                raise RuntimeError(f"merge cycle at {concept_id}")
            seen.add(concept_id)
            concept_id = merges[concept_id]
        return concept_id

    merges = {loser: winner(target) for loser, target in merges.items()}
    losers = set(merges)

    # A moved child must not collide with an existing child of its destination.
    # Pre-existing near-duplicates elsewhere are audited separately and should
    # not prevent this focused reconciliation from being generated.
    for loser in losers:
        target = merges[loser]
        existing = {
            normalized(renames.get(child, concepts[child])): child
            for child in children.get(target, [])
            if child not in losers and child not in deletes
        }
        for child in children.get(loser, []):
            if child in losers or child in deletes:
                continue
            key = normalized(renames.get(child, concepts[child]))
            if key in existing and existing[key] != child:
                raise RuntimeError(
                    f"merge would duplicate siblings under {target}: "
                    f"{existing[key]} and {child} ({key})"
                )

    selectors: dict[str, list[tuple[str, str, int]]] = defaultdict(list)
    labels: dict[str, list[tuple[str, int]]] = defaultdict(list)
    for cid, system, value, ordinal in selector_rows:
        selectors[cid].append((system, value, ordinal))
    for cid, value, ordinal in label_rows:
        labels[cid].append((value, ordinal))

    added_selectors: list[tuple[str, str, str, int]] = []
    added_labels: list[tuple[str, str, int]] = []
    selector_state = {
        cid: {
            "values": {(system, value) for system, value, _ in values},
            "next": defaultdict(int),
        }
        for cid, values in selectors.items()
    }
    for cid, values in selectors.items():
        for system, _value, ordinal in values:
            selector_state[cid]["next"][system] = max(
                selector_state[cid]["next"][system], ordinal + 1
            )
    label_values = {cid: {value for value, _ in values} for cid, values in labels.items()}
    label_next = {
        cid: max((ordinal for _value, ordinal in values), default=-1) + 1
        for cid, values in labels.items()
    }

    for loser in sorted(losers):
        target = merges[loser]
        state = selector_state.setdefault(target, {"values": set(), "next": defaultdict(int)})
        for system, value, _ordinal in selectors.get(loser, []):
            if (system, value) in state["values"]:
                continue
            ordinal = state["next"][system]
            state["next"][system] += 1
            state["values"].add((system, value))
            added_selectors.append((target, system, value, ordinal))
        target_labels = label_values.setdefault(target, set())
        for value in [concepts[loser], *(label for label, _ in labels.get(loser, []))]:
            if value in target_labels:
                continue
            ordinal = label_next.get(target, 0)
            label_next[target] = ordinal + 1
            target_labels.add(value)
            added_labels.append((target, value, ordinal))

    lines = [
        "-- Reconcile duplicate and filing-only structures from the History locality import.",
        "-- Generated by scripts/generate-history-locality-reconciliation.py.",
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

    for loser in sorted(losers):
        target = merges[loser]
        for child in children.get(loser, []):
            if child in losers or child in deletes:
                continue
            if target in parents.get(child, []):
                lines.append(
                    f"DELETE FROM concept_parent WHERE concept_id={sql(child)} "
                    f"AND parent_concept_id={sql(loser)};"
                )
            else:
                lines.append(
                    f"UPDATE concept_parent SET parent_concept_id={sql(target)} "
                    f"WHERE concept_id={sql(child)} AND parent_concept_id={sql(loser)};"
                )
    lines.append("")
    removed = sorted(losers | set(deletes))
    lines.append("DELETE FROM concept_parent WHERE concept_id IN (")
    lines += [
        f"    {sql(cid)}{',' if index + 1 < len(removed) else ''}"
        for index, cid in enumerate(removed)
    ]
    lines.append(");")
    lines.append("DELETE FROM concept WHERE concept_id IN (")
    lines += [
        f"    {sql(cid)}{',' if index + 1 < len(removed) else ''}"
        for index, cid in enumerate(removed)
    ]
    lines += [");", "", "COMMIT;", ""]

    manifest = {
        "version": 1,
        "merges": [
            {
                "loser_id": loser,
                "winner_id": merges[loser],
                "reason": reasons[loser],
            }
            for loser in sorted(losers)
        ],
        "renames": [
            {"concept_id": cid, "preferred_label": value}
            for cid, value in sorted(renames.items())
        ],
        "deleted_ambiguous_span_concepts": deletes,
        "transferred_selectors": len(added_selectors),
        "transferred_labels": len(added_labels),
    }
    args.manifest.write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    args.output.write_text("\n".join(lines), encoding="utf-8")
    print(json.dumps({
        "merged_concepts": len(merges),
        "deleted_ambiguous_span_concepts": len(deletes),
        "renamed_concepts": len(renames),
        "transferred_selectors": len(added_selectors),
        "transferred_labels": len(added_labels),
    }, indent=2))


if __name__ == "__main__":
    main()
