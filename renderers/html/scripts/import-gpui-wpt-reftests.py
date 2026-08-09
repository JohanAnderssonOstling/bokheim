#!/usr/bin/env python3
"""Import unchanged, static WPT reftests and their local resource closure."""

from __future__ import annotations

import argparse
import hashlib
import re
import shutil
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit


DOCUMENT_SUFFIXES = {".html", ".htm", ".xhtml", ".xht", ".svg"}
TEST_SUFFIXES = {".html", ".htm", ".xhtml", ".xht"}
DEFAULT_SCAN_ROOTS = [
    'apng',
    'avif',
    'compat',
    'css',
    'fonts',
    'forced-colors-mode',
    'gif',
    'html',
    'html-ruby-extensions',
    'jpegxl',
    'mathml',
    'png',
    'print',
    'quirks',
    'svg',
    'xml',
]
CSS_URL = re.compile(r"url\(\s*(['\"]?)(.*?)\1\s*\)", re.IGNORECASE)
CSS_IMPORT = re.compile(r"@import\s+(?:url\(\s*)?['\"]([^'\"]+)", re.IGNORECASE)
RESOURCE_ATTRIBUTES = {
    "audio": ("src",),
    "embed": ("src",),
    "feimage": ("href", "xlink:href"),
    "iframe": ("src",),
    "image": ("href", "xlink:href"),
    "img": ("src", "srcset"),
    "input": ("src",),
    "link": ("href",),
    "object": ("data",),
    "source": ("src", "srcset"),
    "track": ("src",),
    "use": ("href", "xlink:href"),
    "video": ("src", "poster"),
}


class DocumentMetadata(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.interactive = False
        self.references: list[str] = []
        self.resources: list[str] = []
        self.inline_css: list[str] = []
        self.base_href: str | None = None
        self._inside_style = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        tag = tag.lower()
        attributes = {name.lower(): value or "" for name, value in attrs}
        if tag == "script" or "reftest-wait" in attributes.get("class", "").split():
            self.interactive = True
        if tag == "style":
            self._inside_style = True
        if tag == "base" and self.base_href is None and "href" in attributes:
            self.base_href = attributes["href"]
        if style := attributes.get("style"):
            self.inline_css.append(style)

        relation = set(attributes.get("rel", "").lower().split())
        href = attributes.get("href", "")
        if tag == "link" and href and relation.intersection({"match", "mismatch"}):
            self.references.append(href)

        for attribute in RESOURCE_ATTRIBUTES.get(tag, ()):
            value = attributes.get(attribute, "")
            if not value or (tag == "link" and relation.intersection({"match", "mismatch"})):
                continue
            if attribute == "srcset":
                self.resources.extend(candidate.strip().split()[0] for candidate in value.split(",") if candidate.strip())
            else:
                self.resources.append(value)

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self.handle_starttag(tag, attrs)
        self.handle_endtag(tag)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() == "style":
            self._inside_style = False

    def handle_data(self, data: str) -> None:
        if self._inside_style:
            self.inline_css.append(data)


def parse_document(path: Path) -> DocumentMetadata:
    parser = DocumentMetadata()
    source = path.read_text(encoding="utf-8", errors="replace")
    parser.feed(source)
    lowered = source.lower()
    parser.interactive |= '<script' in lowered or 'reftest-wait' in lowered
    return parser


def local_path(root: Path, base: Path, raw_url: str) -> Path | None:
    return local_path_from_directory(root, base.parent, raw_url)


def local_path_from_directory(root: Path, base_directory: Path, raw_url: str) -> Path | None:
    raw_url = raw_url.strip()
    if not raw_url or raw_url.startswith(('#', '//')) or '{{' in raw_url:
        return None
    split = urlsplit(raw_url)
    if split.scheme or split.netloc:
        return None
    path = unquote(split.path)
    if not path:
        return None
    target = root / path.lstrip('/') if path.startswith('/') else base_directory / path
    target = target.resolve()
    try:
        target.relative_to(root)
    except ValueError:
        return None
    return target


def document_resource_directory(root: Path, document: Path, base_href: str | None) -> Path:
    if base_href is None:
        return document.parent
    split = urlsplit(base_href.strip())
    if split.scheme or split.netloc or not split.path:
        return document.parent
    path = unquote(split.path)
    target = root / path.lstrip('/') if path.startswith('/') else document.parent / path
    target = target.resolve()
    try:
        target.relative_to(root)
    except ValueError:
        return document.parent
    return target if path.endswith('/') else target.parent


def css_resources(source: str) -> list[str]:
    return [match[1] for match in CSS_URL.findall(source)] + CSS_IMPORT.findall(source)


def candidate_reftests(upstream: Path, scan_roots: list[str]) -> tuple[list[Path], dict[Path, list[Path]]]:
    candidates: list[Path] = []
    references_by_test: dict[Path, list[Path]] = {}
    for scan_root in scan_roots:
        root = upstream / scan_root
        if not root.is_dir():
            continue
        for test in root.rglob('*'):
            if not test.is_file() or test.suffix.lower() not in TEST_SUFFIXES or '.sub.' in test.name:
                continue
            metadata = parse_document(test)
            if metadata.interactive or not metadata.references:
                continue
            references: list[Path] = []
            valid = True
            for href in metadata.references:
                reference = local_path(upstream, test, href)
                if reference is None or not reference.is_file() or '.sub.' in reference.name:
                    valid = False
                    break
                if reference.suffix.lower() in DOCUMENT_SUFFIXES and parse_document(reference).interactive:
                    valid = False
                    break
                references.append(reference)
            if valid:
                candidates.append(test)
                references_by_test[test] = references
    candidates.sort(key=lambda path: path.relative_to(upstream).as_posix())
    return candidates, references_by_test


def resource_closure(upstream: Path, tests: list[Path], references_by_test: dict[Path, list[Path]]) -> set[Path]:
    closure = set(tests)
    for test in tests:
        closure.update(references_by_test[test])
    pending = list(closure)
    inspected: set[Path] = set()
    while pending:
        resource = pending.pop()
        if resource in inspected or not resource.is_file():
            continue
        inspected.add(resource)
        suffix = resource.suffix.lower()
        urls: list[str] = []
        if suffix in DOCUMENT_SUFFIXES:
            metadata = parse_document(resource)
            urls.extend(metadata.resources)
            for inline in metadata.inline_css:
                urls.extend(css_resources(inline))
            resource_directory = document_resource_directory(upstream, resource, metadata.base_href)
        elif suffix == '.css':
            urls.extend(css_resources(resource.read_text(encoding='utf-8', errors='replace')))
            resource_directory = resource.parent
        else:
            resource_directory = resource.parent
        for url in urls:
            dependency = local_path_from_directory(upstream, resource_directory, url)
            if dependency is not None and dependency.is_file() and dependency not in closure:
                closure.add(dependency)
                pending.append(dependency)
    return closure


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def import_files(upstream: Path, destination: Path, files: set[Path]) -> None:
    checksum_path = destination / 'SHA256SUMS'
    checksums: dict[str, str] = {}
    if checksum_path.is_file():
        for line in checksum_path.read_text().splitlines():
            digest, relative = line.split('  ', 1)
            checksums[relative] = digest

    for source in sorted(files):
        relative = source.relative_to(upstream).as_posix()
        target = destination / relative
        digest = sha256(source)
        if target.exists() and sha256(target) != digest:
            raise RuntimeError(f'existing vendored file differs from pinned upstream: {relative}')
        if not target.exists():
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        checksums[relative] = digest

    checksum_path.write_text(''.join(f'{checksums[path]}  {path}\n' for path in sorted(checksums)))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--upstream', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--scan-root', action='append', dest='scan_roots')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    upstream = args.upstream.resolve()
    destination = args.destination.resolve()

    tests, references_by_test = candidate_reftests(upstream, args.scan_roots or DEFAULT_SCAN_ROOTS)
    closure = resource_closure(upstream, tests, references_by_test)
    print(f'static reftests={len(tests)} resource-closure-files={len(closure)}')
    if args.dry_run:
        return

    import_files(upstream, destination, closure)
    manifest = args.manifest.resolve() if args.manifest else destination / 'gpui-reftests.txt'
    manifest.parent.mkdir(parents=True, exist_ok=True)
    entries = [test.relative_to(upstream).as_posix() for test in tests]
    manifest.write_text(
        '# Unchanged, script-free WPT reftests rendered through the production GPUI adapter.\n'
        f'# count: {len(entries)}\n'
        + ''.join(f'{entry}\n' for entry in entries)
    )


if __name__ == '__main__':
    main()
