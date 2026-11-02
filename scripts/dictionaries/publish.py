#!/usr/bin/env python3
"""Publish pinned upstream ZIPs and canonical StarDict dictionary packs."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import subprocess
import tempfile
from urllib.parse import urlsplit
import urllib.request
import zipfile


CATALOG = Path(__file__).resolve().parents[2] / "shared/dictionary-catalog/data/curated.json"
MAX_BYTES = 256 * 1024 * 1024
CHUNK_BYTES = 64 * 1024


def load_catalog(path=CATALOG):
    catalog = json.loads(path.read_text())
    ids = set()
    for spec in catalog:
        if not re.fullmatch(r"[A-Za-z0-9.-]+", spec["id"]) or spec["id"] in ids:
            raise ValueError("Dictionary IDs must be unique release identifiers")
        ids.add(spec["id"])
        for archive in [spec] + ([spec["prepared"]] if spec.get("prepared") else []):
            if not re.fullmatch(r"[0-9a-f]{64}", archive["sha256"]):
                raise ValueError(f"Invalid SHA-256 for {spec['id']}")
            if type(archive["bytes"]) is not int or not 0 < archive["bytes"] <= MAX_BYTES:
                raise ValueError(f"Invalid archive size for {spec['id']}")
        if urlsplit(spec["upstream_url"]).scheme != "https":
            raise ValueError(f"Upstream URL must use HTTPS for {spec['id']}")
        if not spec["license"] or not spec["source_url"].startswith("https://"):
            raise ValueError(f"Source and license are required for {spec['id']}")
    if not catalog:
        raise ValueError("The dictionary catalog is empty")
    return catalog


class HttpsRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        if urlsplit(url).scheme != "https":
            raise ValueError("Dictionary downloads must stay on HTTPS")
        return super().redirect_request(request, response, code, message, headers, url)


def open_archive(url):
    request = urllib.request.Request(url, headers={"User-Agent": "Bokheim-dictionary-publisher/1"})
    return urllib.request.build_opener(HttpsRedirects()).open(request, timeout=60)


def checked_copy(source, spec, destination=None):
    digest = hashlib.sha256()
    size = 0
    while chunk := source.read(CHUNK_BYTES):
        size += len(chunk)
        if size > spec["bytes"]:
            raise ValueError(f"{spec['id']}: archive exceeds the pinned size")
        digest.update(chunk)
        if destination is not None:
            destination.write(chunk)
    if size != spec["bytes"] or digest.hexdigest() != spec["sha256"]:
        raise ValueError(f"{spec['id']}: archive size or SHA-256 does not match the catalog")


def publish_dictionary(spec, directory, verify_only=False):
    target = directory / f"{spec['sha256']}.zip"
    if target.is_symlink():
        raise ValueError(f"Refusing a symlink at {target}")
    if target.exists():
        with target.open("rb") as existing:
            checked_copy(existing, spec)
        return False
    if verify_only:
        raise ValueError(f"Missing published archive: {target}")

    # A failed or interrupted transfer cannot become a publicly named ZIP.
    directory.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=".dictionary-", suffix=".pending", dir=directory)
    pending = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as destination, open_archive(spec["upstream_url"]) as source:
            length = source.headers.get("Content-Length")
            if length is not None and int(length) > spec["bytes"]:
                raise ValueError(f"{spec['id']}: archive exceeds the pinned size")
            checked_copy(source, spec, destination)
            destination.flush()
            os.fsync(destination.fileno())
            os.fchmod(destination.fileno(), 0o644)
        os.replace(pending, target)
    finally:
        pending.unlink(missing_ok=True)
    return True


def validate_prepared_archive(path, spec):
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        members = set(names)
        required = {"dictionary.ifo", "dictionary.idx"}
        bodies = members & {"dictionary.dict", "dictionary.dict.dz"}
        if not required.issubset(members) or len(bodies) != 1:
            raise ValueError(f"{spec['id']}: pack needs original StarDict metadata, index, and exactly one body")
        allowed = required | bodies | {"dictionary.syn", "dictionary.license.txt", "dictionary.attribution.txt"}
        if members - allowed or len(names) != len(members):
            raise ValueError(f"{spec['id']}: pack has unexpected or duplicate components")


def prepare_dictionary(spec, directory, tool, verify_only=False):
    prepared = spec.get("prepared")
    if prepared:
        target = directory / f"{prepared['sha256']}.zip"
        if target.is_symlink():
            raise ValueError(f"Refusing a symlink at {target}")
        if target.exists():
            with target.open("rb") as existing:
                checked_copy(existing, dict(prepared, id=spec["id"]))
            validate_prepared_archive(target, spec)
            return prepared
    if verify_only:
        raise ValueError(f"Missing prepared archive for {spec['id']}")
    if tool is None:
        raise ValueError("A --prepare-tool executable is required to build prepared dictionaries")
    descriptor, name = tempfile.mkstemp(prefix=".prepared-", suffix=".pending", dir=directory)
    os.close(descriptor)
    pending = Path(name)
    try:
        subprocess.run([str(tool.resolve()), str(directory / f"{spec['sha256']}.zip"), str(pending)], check=True)
        size = pending.stat().st_size
        if not 0 < size <= MAX_BYTES:
            raise ValueError(f"{spec['id']}: prepared archive is too large")
        validate_prepared_archive(pending, spec)
        digest = hashlib.sha256(pending.read_bytes()).hexdigest()
        result = dict(bytes=size, sha256=digest)
        if prepared and result != prepared:
            raise ValueError(f"{spec['id']}: preparation does not reproduce the pinned archive")
        target = directory / f"{digest}.zip"
        if target.is_symlink():
            raise ValueError(f"Refusing a symlink at {target}")
        with pending.open("rb") as archive:
            os.fsync(archive.fileno())
        pending.chmod(0o644)
        os.replace(pending, target)
        return result
    finally:
        pending.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="Persistent directory served at /dictionaries/")
    parser.add_argument("--id", action="append", help="Publish only this catalog ID (repeatable); default: all")
    parser.add_argument("--verify-only", action="store_true", help="Check published files without downloading or writing")
    parser.add_argument("--prepare-tool", type=Path, help="StarDict pack executable; validates and canonicalizes original files")
    parser.add_argument("--write-catalog", type=Path, help="Write a catalog containing the prepared archive pins")
    args = parser.parse_args()
    try:
        catalog = load_catalog()
        complete_catalog = catalog
        if args.id:
            unknown = set(args.id) - {spec["id"] for spec in catalog}
            if unknown:
                raise ValueError(f"Unknown dictionary IDs: {', '.join(sorted(unknown))}")
            catalog = [spec for spec in catalog if spec["id"] in args.id]
        for spec in catalog:
            published = publish_dictionary(spec, args.output, args.verify_only)
            if args.prepare_tool or spec.get("prepared"):
                spec["prepared"] = prepare_dictionary(spec, args.output, args.prepare_tool, args.verify_only)
            print(f"{'Published' if published else 'Verified'} {spec['id']}: {spec['sha256']}.zip", flush=True)
        if args.write_catalog:
            if args.verify_only:
                raise ValueError("--write-catalog cannot be combined with --verify-only")
            args.write_catalog.write_text(json.dumps(complete_catalog, ensure_ascii=False, indent=2) + "\n")
        print(f"Verified {len(catalog)} dictionaries ({sum(spec['bytes'] for spec in catalog):,} bytes). Older archives retained.")
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Dictionary publication failed: {error}\n")


if __name__ == "__main__":
    main()
