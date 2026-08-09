#!/usr/bin/env python3
"""Inventory declarative WPT check-layout candidates without executing JS."""

from __future__ import annotations

import argparse
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
import re
import shutil


HTML_SUFFIXES = {".html", ".htm", ".xhtml", ".xht"}
EXPECTED_CHECK = re.compile(r"data-(?:expected|offset)-(?:[a-z-]+)\s*=", re.IGNORECASE)
SCRIPT = re.compile(r"<script(?P<attrs>[^>]*)>(?P<body>.*?)</script\s*>", re.IGNORECASE | re.DOTALL)
SRC = re.compile(r"\bsrc\s*=\s*(['\"])(?P<value>.*?)\1", re.IGNORECASE | re.DOTALL)
COMMENT = re.compile(r"(?:/\*.*?\*/|//[^\n\r]*)", re.DOTALL)
SAFE_EXTERNAL_SCRIPT_SUFFIXES = (
    "/resources/testharness.js",
    "/resources/testharnessreport.js",
    "/resources/check-layout-th.js",
    "/resources/check-layout.js",
    "/support/test-common.js",
    "/check-layout-th.js",
    "/check-layout.js",
)


class EventHandlerCollector(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.handlers: list[tuple[str, str, str]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        for name, value in attrs:
            if name.lower().startswith("on"):
                self.handlers.append((tag.lower(), name.lower(), value or ""))


def normalize_script(source: str) -> str:
    source = COMMENT.sub("", source)
    return re.sub(r"\s+", " ", source).strip().rstrip(";").strip()


def safe_inline_script(source: str) -> bool:
    script = normalize_script(source)
    if not script:
        return True
    # Harness setup only changes when the external checker reports completion.
    script = re.sub(r"\bsetup\s*\(\s*\{\s*explicit_done\s*:\s*true\s*\}\s*\)\s*;?", "", script)
    script = script.strip().rstrip(";").strip()
    if not script:
        return True
    if re.fullmatch(r"checkLayout(?:ForAnchorPos)?\s*\([^;]*\)", script):
        return True
    calls_only = re.sub(r"(?:checkLayout(?:ForAnchorPos)?\s*\([^;]*\)|done\s*\(\s*\))\s*;?", "", script)
    if not calls_only.strip():
        return True
    # Font readiness is irrelevant with the deterministic test shaper.
    if "document.fonts.ready" in script and "checkLayout" in script:
        residue = re.sub(r"document\.fonts\.ready", "", script)
        if not re.search(r"\b(?:style|setAttribute|append|remove|classList|createElement|innerHTML|textContent)\b", residue):
            return True
    return False


def safe_event_handler(tag: str, name: str, source: str) -> bool:
    if tag != "body" or name != "onload":
        return False
    return safe_inline_script(source)


def classify(path: Path) -> tuple[str, str]:
    source = path.read_text(errors="replace")
    unknown_external: list[str] = []
    unsafe_inline: list[str] = []
    for match in SCRIPT.finditer(source):
        src = SRC.search(match.group("attrs"))
        if src:
            value = src.group("value").split("?", 1)[0]
            normalized = "/" + value.lstrip("/")
            if not normalized.endswith(SAFE_EXTERNAL_SCRIPT_SUFFIXES):
                unknown_external.append(value)
        elif not safe_inline_script(match.group("body")):
            unsafe_inline.append(normalize_script(match.group("body"))[:160])

    parser = EventHandlerCollector()
    parser.feed(source)
    unsafe_handlers = [f"{tag}.{name}={value[:120]}" for tag, name, value in parser.handlers if not safe_event_handler(tag, name, value)]
    if unknown_external:
        return "external-script", ", ".join(unknown_external)
    if unsafe_handlers:
        return "event-handler", ", ".join(unsafe_handlers)
    if unsafe_inline:
        return "inline-script", " || ".join(unsafe_inline)
    return "safe", ""


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("upstream", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--show", choices=("safe", "external-script", "event-handler", "inline-script"))
    parser.add_argument("--vendor-safe-to", type=Path, help="copy safe tests unchanged and update --manifest")
    args = parser.parse_args()

    selected: set[str] = set()
    if args.manifest:
        selected = {line for line in args.manifest.read_text().splitlines() if line and not line.startswith("#")}

    counts: Counter[str] = Counter()
    results: list[tuple[str, str, str]] = []
    for path in args.upstream.rglob("*"):
        if not path.is_file() or path.suffix.lower() not in HTML_SUFFIXES:
            continue
        source = path.read_text(errors="replace")
        if not EXPECTED_CHECK.search(source):
            continue
        relative = path.relative_to(args.upstream).as_posix()
        status, detail = classify(path)
        if relative in selected:
            status = "selected"
        counts[status] += 1
        results.append((relative, status, detail))

    if args.show:
        for relative, status, detail in sorted(results):
            if status == args.show:
                print(f"{relative}|{detail}" if detail else relative)
    else:
        for status, count in sorted(counts.items()):
            print(f"{status}: {count}")
        print(f"total: {sum(counts.values())}")

    if args.vendor_safe_to:
        if not args.manifest:
            parser.error("--vendor-safe-to requires --manifest")
        imported = []
        for relative, status, _detail in results:
            # tools/ contains benchmark copies rather than runnable WPT tests.
            if status != "safe" or relative.startswith("tools/"):
                continue
            destination = args.vendor_safe_to / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(args.upstream / relative, destination)
            imported.append(relative)
        comments = [line for line in args.manifest.read_text().splitlines() if not line or line.startswith("#")]
        entries = selected | set(imported)
        args.manifest.write_text("\n".join(comments + sorted(entries)) + "\n")
        print(f"vendored {len(imported)} safe tests; manifest now selects {len(entries)} files")


if __name__ == "__main__":
    main()
