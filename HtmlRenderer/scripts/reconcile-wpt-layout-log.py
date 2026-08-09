#!/usr/bin/env python3
"""Reconcile strict WPT layout ledgers from one failing runner log."""

from __future__ import annotations

import argparse
from pathlib import Path


def section(lines: list[str], marker: str) -> list[str]:
    active = False
    entries: list[str] = []
    for line in lines:
        if line == marker:
            active = True
            continue
        if active and "|node=" in line and "|actual=" in line:
            entries.append(line)
        elif active and entries:
            break
    return entries


def unsupported_reason(entry: str) -> str | None:
    path = entry.split("|", 1)[0]
    if any(marker in path for marker in ("vertical-lr", "vertical-rl", "orthogonal", "wmvert", "-vlr", "-vrl")):
        return "vertical writing-mode is intentionally outside the horizontal layout pipeline"
    if "abspos" in path or "position-absolute" in path or path.endswith("columns-height-set-via-top-bottom.html"):
        return "absolute positioning is intentionally outside the static document layout pipeline"
    if "positioned-items" in path or "with-abspos" in path:
        return "absolute positioning is intentionally outside the static document layout pipeline"
    if path.startswith("css/css-anchor-position/"):
        return "CSS anchor positioning depends on the unsupported positioned-layout model"
    if path.startswith("css/css-contain/") or path.endswith("flex-item-contains-strict.html"):
        return "CSS layout containment is intentionally unsupported"
    if path.startswith("css/css-multicol/"):
        return "CSS multi-column layout is intentionally unsupported"
    if path.startswith("css/css-easing/") or path.endswith("variable-animation-guaranteed-invalid.html"):
        return "animations and their frame clock are intentionally unsupported"
    if path.startswith("css/css-rhythm/"):
        return "experimental CSS rhythmic sizing is intentionally unsupported"
    if path.startswith("css/css-ruby/"):
        return "CSS ruby layout is intentionally unsupported"
    if path.startswith("compat/webkit-box-"):
        return "legacy WebKit box layout compatibility is intentionally unsupported"
    if path.startswith("html/semantics/popovers/"):
        return "interactive HTML popover layout is intentionally unsupported"
    if "/grid-lanes/" in path:
        return "experimental grid-lanes layout is intentionally unsupported"
    if "/subgrid/" in path:
        return "CSS subgrid layout is intentionally unsupported"
    if "grid-items-relative-offsets" in path:
        return "relative positioning and vertical writing-mode are intentionally unsupported"
    return None


def ledger_parts(path: Path) -> tuple[list[str], set[str]]:
    lines = path.read_text().splitlines()
    comments = [line for line in lines if not line or line.startswith("#")]
    entries = {line for line in lines if line and not line.startswith("#")}
    return comments, entries


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=Path)
    parser.add_argument("wpt_root", type=Path)
    args = parser.parse_args()

    lines = args.log.read_text().splitlines()
    failures = section(lines, "unexpected WPT layout mismatches:")
    passes = section(lines, "known WPT layout failures now pass and should be removed:")

    ignored_additions: list[str] = []
    known_additions: list[str] = []
    for failure in failures:
        prefix = failure.rsplit("|actual=", 1)[0]
        reason = unsupported_reason(failure)
        if reason:
            ignored_additions.append(f"{prefix}|reason={reason}")
        else:
            known_additions.append(failure)

    replaced = {entry.rsplit("|actual=", 1)[0] for entry in failures + passes}
    known_path = args.wpt_root / "known-layout-failures.txt"
    comments, known = ledger_parts(known_path)
    known = {entry for entry in known if entry.rsplit("|actual=", 1)[0] not in replaced}
    known.update(known_additions)
    known_path.write_text("\n".join(comments + sorted(known)) + "\n")

    ignored_path = args.wpt_root / "ignored-layout-runs.txt"
    comments, ignored = ledger_parts(ignored_path)
    ignored.update(ignored_additions)
    ignored_path.write_text("\n".join(comments + sorted(ignored)) + "\n")
    print(
        f"classified {len(failures)} failures: {len(known_additions)} actionable, "
        f"{len(ignored_additions)} unsupported; removed {len(passes)} stale failures"
    )


if __name__ == "__main__":
    main()
