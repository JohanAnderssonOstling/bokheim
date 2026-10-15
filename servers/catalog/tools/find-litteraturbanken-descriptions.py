#!/usr/bin/env python3
"""Find review candidates in author presentations for books lacking descriptions.

Read-only: matching a title is evidence of a mention, not a usable book synopsis.
Retains fetched source documents and their URL, timestamp, and SHA-256 receipts.
Uses the same author/presentation endpoints as Litteraturbanken's frontend.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sqlite3
import time
import unicodedata
from urllib.parse import quote, unquote, urlsplit
from urllib.request import Request, urlopen


ORIGIN = "https://litteraturbanken.se"


def normalized(text):
    text = unicodedata.normalize("NFKC", text).casefold()
    return " ".join(re.findall(r"\w+", text))


class Paragraphs(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.blocks = []
        self.parts = None
        self.links = []
        self.block_links = []
        self.hidden = 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style"):
            self.hidden += 1
        if tag == "p":
            self.flush()
            self.parts = []
            self.links = []
        if tag == "a" and self.parts is not None:
            href = dict(attrs).get("href")
            if href:
                self.links.append(href)
        if tag == "br" and self.parts is not None:
            self.parts.append(" ")

    def handle_endtag(self, tag):
        if tag in ("script", "style"):
            self.hidden = max(0, self.hidden - 1)
        if tag == "p":
            self.flush()

    def handle_data(self, data):
        if self.parts is not None and not self.hidden:
            self.parts.append(data)

    def flush(self):
        if self.parts:
            value = " ".join("".join(self.parts).split())
            if value:
                self.blocks.append(value)
                self.block_links.append(self.links[:])
        self.parts = None


def paragraphs(html):
    parser = Paragraphs()
    parser.feed(html)
    parser.flush()
    return parser.blocks


def document(url, kind, html):
    parser = Paragraphs()
    parser.feed(html)
    parser.flush()
    return {"url": url, "kind": kind, "paragraphs": parser.blocks,
            "paragraph_links": parser.block_links}


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def fetch(output, name, url):
    path = output / name
    receipt = path.with_suffix(path.suffix + ".http.json")
    if path.exists() and receipt.exists():
        body = path.read_bytes()
        saved = json.loads(receipt.read_text())
        if saved["url"] != url or saved["sha256"] != hashlib.sha256(body).hexdigest():
            raise ValueError(f"Cached evidence mismatch: {path}")
        return body
    request = Request(url, headers={"User-Agent": "bokheim-catalog-survey",
                                   "Referer": ORIGIN + "/epub"})
    for attempt in range(3):
        try:
            with urlopen(request, timeout=25) as response:
                body = response.read()
                final_url = response.url
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(body)
            write_json(receipt, {"url": url, "final_url": final_url,
                                "fetched_at": datetime.now(timezone.utc).isoformat(),
                                "bytes": len(body), "sha256": hashlib.sha256(body).hexdigest()})
            return body
        except Exception:
            if attempt == 2:
                raise
            time.sleep(attempt + 1)


def missing_books(database):
    with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
        connection.row_factory = sqlite3.Row
        rows = connection.execute("""
            SELECT s.entry_id, s.entry_url, s.book_hash, s.external_entry_key,
                   coalesce(b.title,e.title) AS title, coalesce(b.author,e.author) AS author
            FROM source_entry s
            LEFT JOIN book b ON b.hash=s.book_hash
            LEFT JOIN external_book e ON e.entry_key=s.external_entry_key
            WHERE s.source='litteraturbanken.se'
              AND (b.hash IS NOT NULL OR e.entry_key IS NOT NULL)
              AND trim(coalesce(json_extract(coalesce(b.record,e.record),'$.description'),''))=''
            ORDER BY title,s.entry_id
        """).fetchall()
    books = [dict(row) for row in rows]
    for book in books:
        parts = unquote(urlsplit(book["entry_url"]).path).split("/")
        book["authorid"] = parts[2]
        book["titlepath"] = parts[4]
    return books


def author_documents(output, authorid):
    api_url = ORIGIN + "/api/get_author/" + quote(authorid)
    author = json.loads(fetch(output, f"authors/{authorid}.json", api_url))["data"]
    if not author:
        return {"authorid": authorid, "error": "No author record", "documents": []}
    documents = []
    if author.get("intro"):
        documents.append(document(api_url, "short_author_introduction", author["intro"]))
    if author.get("presentation"):
        norm = author["authorid_norm"]
        url = ORIGIN + f"/red/forfattare/{quote(norm)}/presentation/index.html"
        try:
            html = fetch(output, f"presentations/{norm}.html", url).decode("utf-8-sig")
            documents.append(document(url, "full_author_presentation", html))
        except Exception as error:
            return {"authorid": authorid, "error": str(error), "documents": documents}
    return {"authorid": authorid, "authorid_norm": author.get("authorid_norm"),
            "documents": documents}


def candidates(book, documents):
    # Do not shorten titles to generic words (Dikter, Brev, etc.) or discard
    # volume numbers. Such matches need explicit review against the edition.
    title = normalized(book["title"])
    hits = []
    for document in documents:
        for index, paragraph in enumerate(document["paragraphs"]):
            linked = any(
                # Match the author and complete title path, including volume.
                unquote(urlsplit(link).path).startswith(
                    f"/{prefix}/{authorid}/titlar/{titlepath}/")
                for link in document["paragraph_links"][index]
                for prefix in ("forfattare", "författare")
                for authorid in book.get("author_aliases", [book["authorid"]])
                for titlepath in book.get("titlepath_aliases", [book["titlepath"]]))
            named = len(title) >= 5 and f" {title} " in f" {normalized(paragraph)} "
            if named or linked:
                hits.append({"source_url": document["url"], "kind": document["kind"],
                             "paragraph_index": index, "text": paragraph,
                             "matched_by": "explicit_work_link" if linked else "title_mention",
                             "requires_review": True})
    return hits


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--listing", type=Path,
                        help="Optional survey epub-list.json, for website's ASCII title-path aliases")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    books = missing_books(args.database.resolve())
    listing = {}
    if args.listing:
        listing = {row["lbworkid"]: row for row in json.loads(args.listing.read_text())["data"]}
    write_json(args.output / "missing-books.json", books)
    authors = {}
    authorids = sorted({book["authorid"] for book in books})
    with ThreadPoolExecutor(max_workers=3) as pool:
        futures = {pool.submit(author_documents, args.output, authorid): authorid
                   for authorid in authorids}
        for future in as_completed(futures):
            authorid = futures[future]
            try:
                authors[authorid] = future.result()
            except Exception as error:
                authors[authorid] = {"authorid": authorid, "error": str(error), "documents": []}
            if len(authors) % 25 == 0:
                print(f"Checked {len(authors)}/{len(authorids)} authors", flush=True)
    for book in books:
        author_norm = authors[book["authorid"]].get("authorid_norm")
        book["author_aliases"] = sorted({book["authorid"], author_norm} - {None})
        title_norm = listing.get(book["entry_id"], {}).get("titlepath_norm")
        book["titlepath_aliases"] = sorted({book["titlepath"], title_norm} - {None})
    results = [{**book, "candidates": candidates(book, authors[book["authorid"]]["documents"])}
               for book in books]
    write_json(args.output / "author-documents.json", authors)
    write_json(args.output / "candidates.json", results)
    summary = {"missing_books_checked": len(books), "authors_checked": len(authors),
               "full_presentations_fetched": sum(any(d["kind"] == "full_author_presentation"
                   for d in author["documents"]) for author in authors.values()),
               "books_with_title_mentions": sum(bool(book["candidates"]) for book in results),
               "fetch_errors": [{"authorid": a, "error": v["error"]}
                                for a,v in authors.items() if v.get("error")],
               "catalog_modified": False,
               "caveat": "Title mentions are review candidates, not verified descriptions."}
    write_json(args.output / "summary.json", summary)
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
