#!/usr/bin/env python3
"""One-off metadata survey; writes evidence, never changes the catalog.

Usage: python servers/catalog/tools/survey-litteraturbanken.py OUTPUT_DIRECTORY
The output includes the public metadata listing and eight EPUB comparisons.
Only CC0 sample files are retained; other samples are inspected in memory.
"""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
from html.parser import HTMLParser
import io
import json
from pathlib import Path
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET
from zipfile import ZipFile


ORIGIN = "https://litteraturbanken.se"
SAMPLES = [
    ("BoyeK", "Kallocain"),
    ("SöderbergH", "DoktorGlas"),
    ("LagerlöfS", "GöstaBerling1"),
    ("SödergranE", "Dikter"),
    ("StrindbergA", "RödaRummet"),
    ("MartinsonH", "Aniara"),
    ("ForsskålP", "ThoughtsOnCivilLiberty"),
    ("JalviP", "Muottacalmit"),
]


class VisibleText(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.hidden = 0
        self.parts = []

    def handle_starttag(self, tag, attrs):
        if tag in ("style", "script"):
            self.hidden += 1

    def handle_endtag(self, tag):
        if tag in ("style", "script"):
            self.hidden = max(0, self.hidden - 1)

    def handle_data(self, data):
        if not self.hidden:
            self.parts.append(data)


def text(value):
    parser = VisibleText()
    parser.feed(value or "")
    return " ".join(" ".join(parser.parts).split())


def save_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def fetch(output, relative, url, retain=True):
    path = output / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() or path.with_suffix(path.suffix + ".http.json").exists():
        raise ValueError(f"Evidence already exists: {path}; choose a new output directory")
    request = Request(url, headers={
        "User-Agent": "bokheim-catalog-survey",
        "Referer": ORIGIN + "/epub",
    })
    with urlopen(request, timeout=45) as response:
        body = response.read()
        headers = dict(response.headers)
        final_url = response.url
    if retain:
        path.write_bytes(body)
    save_json(path.with_suffix(path.suffix + ".http.json"), {
        "retained": retain,
        "url": url,
        "final_url": final_url,
        "fetched_at": datetime.now(timezone.utc).isoformat(),
        "bytes": len(body),
        "sha256": hashlib.sha256(body).hexdigest(),
        "headers": headers,
    })
    return body


def epub_url(row):
    # The website's services module uses the containing work for part entries.
    author = (row.get("work_authors") or row.get("authors") or [row["main_author"]])[0]
    name = author["authorid"] + "_" + (row.get("work_titleid") or row["titleid"]) + ".epub"
    return ORIGIN + "/txt/epub/" + quote(name)


def inspect_epub(raw):
    with ZipFile(io.BytesIO(raw)) as archive:
        if archive.read("mimetype").strip() != b"application/epub+zip":
            raise ValueError("Download is not an EPUB")
        container = ET.fromstring(archive.read("META-INF/container.xml"))
        package_path = next(n.attrib["full-path"] for n in container.iter()
                            if n.tag.rsplit("}", 1)[-1] == "rootfile")
        package = ET.fromstring(archive.read(package_path))
        metadata = next(n for n in package if n.tag.rsplit("}", 1)[-1] == "metadata")
        declared = [{"name": n.tag.rsplit("}", 1)[-1], "attributes": n.attrib,
                     "value": "".join(n.itertext())} for n in metadata]
        documents = {name: text(archive.read(name).decode("utf-8-sig"))
                     for name in archive.namelist() if name.endswith((".xhtml", ".html"))}
        return declared, documents


def summarize(rows):
    grouped = {}
    for row in rows:
        grouped.setdefault(epub_url(row), []).append(row)
    return {
        "listing_rows": len(rows),
        "distinct_download_urls": len(grouped),
        "generated_part_rows": sum(bool(r.get("generated_from_part")) for r in rows),
        "nonempty_field_counts": dict(sorted(Counter(k for r in rows for k, v in r.items() if v).items())),
        "language_rows": dict(Counter(r.get("language") or "missing" for r in rows)),
        "texttype_rows": dict(Counter(r.get("texttype") or "missing" for r in rows)),
        "epub_license_rows": dict(Counter(r.get("epub_license") or "missing" for r in rows)),
        "description_rows": sum(bool(r.get("workintro_download") or r.get("workintro")) for r in rows),
        "downloads_with_any_description": sum(any(r.get("workintro_download") or r.get("workintro") for r in group)
                                              for group in grouped.values()),
        "duplicate_download_groups": [{"file_url": url, "titlepaths": [r["titlepath"] for r in group]}
                                      for url, group in grouped.items() if len(group) > 1],
    }


def survey(output):
    output.mkdir(parents=True, exist_ok=True)
    # These public endpoints and the URL construction were observed in the
    # site's frontend. They are not a documented, versioned API contract.
    params = {"q": "has_epub:true AND show:true", "from": 0, "to": 2000,
              "exclude": "text,parts,pages,errata,content_vector", "sort_field": "sortkey|asc"}
    listing_url = ORIGIN + "/api/query_string/etext,faksimil,pdf?" + urlencode(params)
    listing = json.loads(fetch(output, "epub-list.json", listing_url))
    rows = listing["data"]
    if len(rows) != listing["hits"]:
        raise ValueError(f"Incomplete listing: {len(rows)} of {listing['hits']}; increase the limit or paginate")
    fetch(output, "licenses.json", ORIGIN + "/red/etc/license/license.json")
    summary = summarize(rows)
    summary["listing_url"] = listing_url
    summary["sample_comparisons"] = []
    for authorid, titlepath in SAMPLES:
        row = next(r for r in rows if r["main_author"]["authorid"] == authorid and r["titlepath"] == titlepath)
        stem = authorid + "_" + titlepath
        work_url = ORIGIN + "/api/get_work_info?" + urlencode({"authorid": authorid, "titlepath": titlepath})
        details = json.loads(fetch(output, "works/" + stem + ".json", work_url))
        work = next(r for r in details["data"] if r["mediatype"] == "etext" and r["lbworkid"] == row["lbworkid"])
        author_url = ORIGIN + "/api/get_author/" + quote(authorid)
        author = json.loads(fetch(output, "authors/" + authorid + ".json", author_url))["data"]
        raw = fetch(output, "epubs/" + stem + ".epub", epub_url(row), retain=row["epub_license"] == "cc-0")
        declared, documents = inspect_epub(raw)
        comparisons = {}
        for field in ("workintro", "workintro_download", "sourcedesc", "cover_download_info"):
            value = text(work.get(field))
            comparisons[field] = {
                "api_present": bool(value),
                "opf_fields_containing_value": [m["name"] for m in declared if value and value in text(m["value"])],
                "epub_documents_containing_value": [name for name, content in documents.items() if value and value in content],
            }
        # Contains checks are exact normalized text checks, not semantic matches.
        summary["sample_comparisons"].append({
            "title": work["title"], "lbworkid": work["lbworkid"],
            "info_url": ORIGIN + "/författare/" + quote(authorid) + "/titlar/" + quote(titlepath, safe="/") + "/info",
            "work_api_url": work_url, "file_url": epub_url(row),
            "api_language": work.get("language"), "imprintyear": work.get("imprintyear"),
            "download_isbn": work.get("isbn_download"), "librisid": work.get("librisid"),
            "keyword": work.get("keyword", []), "texttype": work.get("texttype"),
            "epub_license": work.get("epub_license"), "export": work.get("export", []),
            "download_bytes": len(raw), "opf_metadata": declared,
            "description_comparison": comparisons,
            "author_intro_present": bool(author.get("intro")),
            "author_intro_in_epub": any(text(author.get("intro")) in content for content in documents.values()) if author.get("intro") else False,
            "author_extra_fields": sorted(k for k, v in author.items() if v),
        })
        print(f"Compared {stem}", flush=True)
    save_json(output / "survey.json", summary)
    print(json.dumps({k: summary[k] for k in ("listing_rows", "distinct_download_urls", "description_rows",
                                            "downloads_with_any_description", "language_rows")}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    survey(parser.parse_args().output)
