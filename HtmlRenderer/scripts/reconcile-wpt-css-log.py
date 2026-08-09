#!/usr/bin/env python3
"""Merge exact CSS WPT mismatches from one strict runner log."""

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
        if active and "|line=" in line and "|actual=" in line:
            entries.append(line)
        elif active and entries:
            break
    return entries


def run_prefix(entry: str) -> str:
    return entry.rsplit("|actual=", 1)[0]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=Path)
    parser.add_argument("ledger", type=Path)
    args = parser.parse_args()

    lines = args.log.read_text().splitlines()
    failures = section(lines, "unexpected CSS parsing mismatches:")
    passes = section(lines, "known CSS parsing failures now pass and should be removed:")
    replaced = {run_prefix(entry) for entry in failures + passes}

    ledger_lines = args.ledger.read_text().splitlines()
    comments = [line for line in ledger_lines if not line or line.startswith("#")]
    entries = {line for line in ledger_lines if line and not line.startswith("#") and run_prefix(line) not in replaced}
    entries.update(failures)
    args.ledger.write_text("\n".join(comments + sorted(entries)) + "\n")
    print(f"added {len(failures)} exact failures and removed {len(passes)} stale failures")


if __name__ == "__main__":
    main()
