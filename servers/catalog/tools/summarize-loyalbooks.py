#!/usr/bin/env python3
"""Summarize an operator-collected Loyal Books survey without acquiring books.

Usage: summarize-loyalbooks.py SURVEY_DIRECTORY [--catalog-db CURATION_SQLITE]
Reads sitemap/index seeds and retained browser excerpts; writes JSON/CSV evidence.
The random sample is a uniform sample of sitemap URLs, not of popular titles.
Missing browser responses remain unknown. Every selection requires a rating;
EPUBs have no minimum score, while audiobook links require at least four stars.
"""

import argparse
from collections import Counter, defaultdict
import csv
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import re
import sqlite3
import statistics
import unicodedata
from urllib.parse import urlsplit


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def title_key(title):
    return "".join(c for c in unicodedata.normalize("NFKD", title).casefold() if c.isalnum())


def excerpts(directory):
    collected = defaultdict(list)
    for pattern in ("web-*.txt", "language-details-*.txt", "random-*.txt", "epub-*.txt"):
        for path in sorted(directory.glob(pattern)):
            for chunk in path.read_text().split("--------------------------------------------------------------------------------"):
                source = re.search(r'Source: find\(\{"ref_id"\s*:\s*"([^"]+)"', chunk)
                if source:
                    collected[source[1]].append((path.name, chunk.strip()))
    description_path = directory / "description-evidence.json"
    if description_path.exists():
        for page in json.loads(description_path.read_text())["pages"]:
            collected[page["entry_url"]].append((description_path.name, page["response"]))
    return collected


def visible_text(value):
    def link(match):
        parts = match[1].split("†")
        label = parts[1].strip() if len(parts) > 1 else ""
        label = re.sub(r"(?<!\S)L\d+:\s*", "", label)
        return "" if label.startswith("Image:") else label
    value = re.sub(r"cite([^]+)", link, value)
    value = re.sub(r"\[Image \d+: [^\]]+\]", "", value)
    return value.strip()


def page_description(evidence, author, title):
    lines = {}
    origins = {}
    # The description request supplies the beginning; older download excerpts
    # can supply its last paragraph and boundary when the browser clips context.
    for filename, chunk in sorted(evidence, key=lambda item: item[0] != "description-evidence.json"):
        if "Crawled:" not in chunk or chunk.startswith("Internal Error"):
            continue
        cleaned = visible_text(chunk)
        original_numbers = {int(n) for n in re.findall(r"(?<!\S)L(\d+):", chunk)}
        parsed_numbers = set()
        for match in re.finditer(r"(?<!\S)L(\d+):\s*(.*?)(?=(?<!\S)L\d+:|\Z)", cleaned, re.DOTALL):
            number = int(match[1])
            lines.setdefault(number, visible_text(match[2]))
            origins.setdefault(number, filename)
            parsed_numbers.add(number)
        # The browser sometimes wraps one linked author across numbered lines.
        # Flattening that link moves its text into the preceding line.
        for number in original_numbers - parsed_numbers:
            lines.setdefault(number, "")
            origins.setdefault(number, filename)
    starts = [number for number, value in lines.items() if "By:" in value]
    has_author = bool(starts)
    if starts:
        start = min(starts)
    else:
        headings = [n for n, value in lines.items() if value.startswith("# ") and title_key(value[2:]) == title_key(title)]
        if not headings:
            return None, "pending", []
        start = min(headings) + 1
        while start in lines and lines[start] in ("", "---"):
            start += 1
    boundaries = [number for number, value in lines.items() if number > start and re.match(r"^(?:\*\s*)?#?\s*(?:Stream audiobook|Audiobook downloads|eBook Downloads|Genres for this book|This book is in genre|Book sections)", value)]
    if not boundaries:
        return None, "pending", []
    end = min(boundaries)
    if any(number not in lines for number in range(start, end)):
        return None, "pending", []
    first = lines[start].split("By:", 1)[1].strip() if has_author else lines[start]
    author_name = (author or "").split(" - ", 1)[0]
    if not has_author:
        if first.startswith(title):
            first = first[len(title):].strip()
    elif author_name and first.startswith(author_name):
        first = first[len(author_name):].strip()
        first = re.sub(r"^\(\d{3,4}[^)]*\)\s*", "", first)
    elif re.match(r"^[^()]+\(\d{3,4}[^)]*\)", first):
        first = re.sub(r"^[^()]+\(\d{3,4}[^)]*\)\s*", "", first)
    else:
        return None, "pending", []
    paragraphs = [first] + [lines[number] for number in range(start + 1, end)]
    paragraphs = [p.replace("Book Description:", "").strip() for p in paragraphs]
    paragraphs = [p for p in paragraphs if p and p != "---"]
    description = "\n\n".join(paragraphs)
    return description or None, "captured" if description else "not_available", sorted({origins[number] for number in range(start, end)})


def describe(seed, evidence, download_links=None):
    raw = "\n".join(chunk for _, chunk in evidence)
    # Search terms occur in the tool's Source header even when there is no match.
    # Parse only page lines, otherwise every MP3 query becomes an audio offer.
    body = "\n".join(re.findall(r"^L\d+: (.*)$", raw, re.MULTILINE))
    # Some cached browser documents omit their content-type field.
    valid_chunks = [chunk for _, chunk in evidence if "Crawled:" in chunk and not chunk.startswith("Internal Error")]
    resolved = bool(valid_chunks)
    rating = re.search(r"Rated:\s*([\d.]+) Stars?\s*-\s*(\d+) (?:review|vote)", body)
    size = re.search(r"zip file\s*\(([\d.,]+)\s*(MB|GB)\)", body)
    duration = re.search(r"\((\d+:\d+:\d+) long\)", body)
    advertised = round(float(size[1].replace(",", "")) * (10**9 if size[2] == "GB" else 10**6)) if size else None
    audio = "MP3 Download" in body
    estimated = None
    if audio and advertised is None and duration:
        hours, minutes, seconds = map(int, duration[1].split(":"))
        estimated = (hours * 3600 + minutes * 60 + seconds) * 8000
    epub_checks = [chunk for chunk in valid_chunks if '"pattern":"ePUB eBook"' in chunk]
    epub = True if "ePUB eBook" in body else (False if epub_checks else None)
    header = valid_chunks[0].splitlines()[0] if valid_chunks else ""
    entry_url = urlsplit(seed["entry_url"])
    free = bool(resolved and entry_url.hostname in ("loyalbooks.com", "www.loyalbooks.com") and entry_url.path.startswith("/book/") and "Free at Loyal Books" in header)
    name = header.split(" - Free at Loyal Books", 1)[0]
    name = re.sub(r" - [^-]+ - Free at Loyal Books.*", "", name)
    title = seed.get("title") or (name.split(" by ", 1)[0] if name else seed.get("image_title")) or seed["entry_url"].rsplit("/", 1)[-1]
    author = name.split(" by ", 1)[1] if " by " in name else None
    description, description_status, description_evidence = page_description(evidence, author, title)
    value = float(rating[1]) if rating else None
    reviews = int(rating[2]) if rating else None
    rated = bool(value is not None and 1 <= value <= 5 and reviews and reviews >= 1)
    selected_audio = bool(free and rated and audio and value >= 4)
    popularity = seed.get("popularity", [])
    sitewide = next((entry for entry in popularity if entry["scope"] == "sitewide"), None)
    featured = next((entry for entry in popularity if entry["scope"] == "language_featured"), None)
    listing = sitewide or featured
    download = (download_links or {}).get(seed["entry_url"], {})
    download_url = download.get("download_url") if selected_audio else None
    if download_url:
        target = urlsplit(download_url)
        if target.scheme not in ("http", "https") or target.hostname not in ("archive.org", "www.archive.org") or not target.path.startswith("/download/") or target.username or target.password:
            raise ValueError(f"Unexpected audiobook download URL: {download_url}")
    return {
        "source": "loyalbooks.com", "source_entry_id": entry_url.path.removeprefix("/book/"),
        "entry_url": seed["entry_url"], "title": title, "author": author,
        "description": description, "description_status": description_status,
        "description_source_url": seed["entry_url"] if description is not None else None,
        "description_evidence_files": description_evidence,
        "language_listing": seed.get("language"), "survey_groups": sorted(seed["groups"]),
        "page_resolved": resolved, "rating": value, "review_count": reviews, "has_rating": rated,
        "loyalbooks_popularity": popularity,
        "loyalbooks_popularity_rank": sitewide["rank"] if sitewide else None,
        "loyalbooks_popularity_scope": listing["scope"] if listing else None,
        "loyalbooks_popularity_listing_url": listing["listing_url"] if listing else None,
        "loyalbooks_language_featured_position": featured["position"] if featured else None,
        "free_book": free, "free_evidence": "Loyal Books free-book detail page" if free else None,
        "epub_available": epub, "retain_epub": free and rated and epub is True,
        "mp3_available": audio if resolved else None, "retain_audiobook": selected_audio,
        "audiobook_delivery": "external_download" if selected_audio else None,
        "audiobook_download_url": download_url,
        "audiobook_download_host": "archive.org" if download_url else None,
        "download_link_status": download.get("status", "pending") if selected_audio else None,
        "store_audiobook_file": False, "hosted_audio_file_bytes": 0,
        "audio_decision": "eligible" if selected_audio else ("unresolved" if not resolved else "not_confirmed_free" if not free else "unrated" if not rated else "no_mp3_offer" if not audio else "below_4"),
        "advertised_mp3_bytes": advertised, "estimated_mp3_bytes_64kbps": estimated,
        "storage_bytes": advertised if advertised is not None else estimated,
        "storage_basis": "advertised_zip_size" if advertised is not None else "duration_at_64kbps" if estimated is not None else "unknown",
        "duration": duration[1] if duration else None,
        "listing_rating": seed.get("listing_rating"),
        "listing_rating_differs_from_detail": seed.get("listing_rating") is not None and value is not None and seed["listing_rating"] != value,
        "evidence_files": sorted({name for name, _ in evidence}),
    }


def main(directory, catalog_db):
    inputs = [("top100-listing.json", "popular"), ("language-book-listing.json", "language"), ("random-sample-listing.json", "random")]
    languages = json.loads((directory / "language-counts.json").read_text())
    language_urls = {row["name"]: row["language_url"] for row in languages}
    featured_positions = {}
    click_path = directory / "language-clicks.json"
    if click_path.exists():
        positions = Counter()
        for row in json.loads(click_path.read_text()):
            positions[row["language"]] += 1
            featured_positions[(row["language"], row["ref_id"], row["id"])] = positions[row["language"]]
    survey_date = re.search(r"survey-(\d{4}-\d{2}-\d{2})", directory.name)
    observed_on = survey_date[1] if survey_date else None
    seeds = {}
    for name, group in inputs:
        for row in json.loads((directory / name).read_text()):
            seed = seeds.setdefault(row["entry_url"], {"entry_url": row["entry_url"], "groups": set(), "popularity": []})
            seed["groups"].add(group)
            if group == "popular" and row.get("rank") is not None:
                seed["popularity"].append({"scope": "sitewide", "rank": row["rank"], "listing_url": "https://www.loyalbooks.com/Top_100", "sort": "popularity", "observed_on": observed_on, "evidence_file": name})
            if group == "language":
                position = featured_positions.get((row["language"], row["ref_id"], row["id"]))
                if position is not None:
                    seed["popularity"].append({"scope": "language_featured", "language": row["language"], "position": position, "listing_url": language_urls[row["language"]], "sort": "popularity", "observed_on": observed_on, "evidence_files": [name, click_path.name]})
            for key in ("title", "language", "listing_rating", "image_title"):
                if row.get(key) is not None:
                    seed[key] = row[key]
    evidence = excerpts(directory)
    link_path = directory / "audio-download-links.json"
    download_links = {r["entry_url"]: r for r in json.loads(link_path.read_text())} if link_path.exists() else {}
    rows = [describe(seed, evidence[url], download_links) for url, seed in seeds.items()]
    existing = defaultdict(list)
    if catalog_db:
        with sqlite3.connect(catalog_db.resolve().as_uri() + "?mode=ro", uri=True) as db:
            for book_hash, title in db.execute("SELECT hash,title FROM book"):
                existing[title_key(title)].append(book_hash)
    for row in rows:
        row["possible_existing_epub_title_matches"] = existing[title_key(row["title"])]
    selected = [row for row in rows if row["retain_epub"] or row["retain_audiobook"]]
    audio = [row for row in rows if row["retain_audiobook"]]
    random_rows = [row for row in rows if "random" in row["survey_groups"]]
    resolved_random = [row for row in random_rows if row["page_resolved"]]
    sitemap = json.loads((directory / "sitemap-candidates.json").read_text())
    population = len(sitemap)
    values = [(row["storage_bytes"] or 0) if row["retain_audiobook"] else 0 for row in resolved_random]
    point = population * statistics.mean(values)
    se = population * statistics.stdev(values) / math.sqrt(len(values)) * math.sqrt((population - len(values)) / (population - 1))
    summary = {
        "summarized_at": datetime.now(timezone.utc).isoformat(),
        "policy": {"languages": "all", "free_books_only": True, "rating_required_for_all_books": True, "minimum_rating_count": 1, "epub_minimum_rating": None, "epub_storage": "retain EPUB files", "unrated_books": "exclude", "audiobook_minimum_displayed_rating": 4, "unrated_audiobooks": "exclude", "audiobook_delivery": "external_download", "audio_storage": "download links and metadata only; no local audio files", "source": "loyalbooks.com; download host recorded separately"},
        "scope": {"sitemap_urls": population, "language_listing_pages": len(languages), "advertised_language_audio_count_sum_not_deduplicated": sum(r["count"] or 0 for r in languages), "detail_urls_attempted": len(rows), "detail_urls_resolved": sum(r["page_resolved"] for r in rows), "detail_urls_unresolved": sum(not r["page_resolved"] for r in rows)},
        "candidates": {"total": len(selected), "epubs": sum(r["retain_epub"] for r in rows), "audiobooks": len(audio), "both_formats": sum(r["retain_epub"] and r["retain_audiobook"] for r in rows), "epub_only": sum(r["retain_epub"] and not r["retain_audiobook"] for r in rows), "audio_only": sum(r["retain_audiobook"] and not r["retain_epub"] for r in rows), "possible_existing_epub_title_matches": sum(bool(r["possible_existing_epub_title_matches"]) for r in selected), "possible_existing_epub_title_matches_with_audio": sum(bool(r["possible_existing_epub_title_matches"]) for r in audio)},
        "audiobook_rating_counts": {"strictly_above_4": sum(r["rating"] > 4 for r in audio), "exactly_4": sum(r["rating"] == 4 for r in audio), "at_least_4": len(audio)},
        "candidate_popularity_data": {"sitewide_rank_available": sum(r["loyalbooks_popularity_rank"] is not None for r in selected), "language_featured_position_available": sum(r["loyalbooks_language_featured_position"] is not None for r in selected), "no_popularity_observed": sum(not r["loyalbooks_popularity"] for r in selected), "missing_values": "unknown, not zero", "scope": "source listing positions; no numeric popularity score inferred"},
        "candidate_descriptions": dict(Counter(r["description_status"] for r in selected)),
        "hosted_audio_storage": {"audio_file_bytes": 0, "download_links_recorded": sum(bool(r["audiobook_download_url"]) for r in audio), "download_links_pending": sum(not r["audiobook_download_url"] for r in audio)},
        "candidate_audio_payload": {"purpose": "external download sizes, not local catalog storage", "advertised_bytes": sum(r["advertised_mp3_bytes"] or 0 for r in audio), "duration_estimated_bytes": sum(r["estimated_mp3_bytes_64kbps"] or 0 for r in audio), "total_bytes": sum(r["storage_bytes"] or 0 for r in audio), "estimated_recordings": sum(r["estimated_mp3_bytes_64kbps"] is not None for r in audio), "unknown_size_recordings": sum(r["storage_bytes"] is None for r in audio)},
        "sitewide_sampling_estimate": {
            "random_seed": 20261012, "sample_urls": len(random_rows), "resolved_urls": len(resolved_random), "unresolved_urls": len(random_rows) - len(resolved_random),
            "observed_eligible_recordings": sum(r["retain_audiobook"] for r in random_rows),
            "response_weighted_eligible_url_estimate": population * sum(r["retain_audiobook"] for r in random_rows) / len(resolved_random),
            "response_weighted_audio_bytes_estimate": round(point), "conditional_standard_error_bytes": round(se),
            "conditional_approx_95_percent_interval_bytes": [round(max(0, point - 1.96 * se)), round(point + 1.96 * se)],
            "zero_unresolved_contribution_scenario_bytes": round(population * sum(values) / len(random_rows)),
            "purpose": "historical external payload estimate; link-only policy requires no hosted audio files",
            "assumptions": ["Unresolved pages have the same mean eligible audio size as resolved pages for the response-weighted estimate and interval; this may be wrong.", "Zero unresolved contribution is a sensitivity scenario, not a measured lower bound on the whole site.", "A sitemap URL is treated as one recording; audio identities have not been deduplicated across different book pages.", "Missing advertised sizes use duration at 64 kbps; ZIP sizes are rounded source claims, not measured downloads.", "Browser evidence includes cached pages of different ages. Refresh ratings and verify Archive file identities and sizes before acquisition.", "Totals exclude EPUBs, covers, backup copies, staging downloads and additional audio encodings."]},
        "audio_decisions": dict(Counter(r["audio_decision"] for r in rows)),
        "listing_detail_rating_disagreements": sum(r["listing_rating_differs_from_detail"] for r in rows),
        "catalog_mutated": False,
    }
    save(directory / "survey.json", summary)
    save(directory / "reviewed-pages.json", rows)
    save(directory / "candidates.json", selected)
    save(directory / "audiobook-candidates.json", audio)
    fields = ["source", "source_entry_id", "title", "author", "description", "description_status", "description_source_url", "language_listing", "free_book", "has_rating", "rating", "review_count", "loyalbooks_popularity_rank", "loyalbooks_popularity_scope", "loyalbooks_popularity_listing_url", "loyalbooks_language_featured_position", "retain_epub", "retain_audiobook", "audiobook_delivery", "audiobook_download_url", "audiobook_download_host", "download_link_status", "store_audiobook_file", "hosted_audio_file_bytes", "storage_bytes", "storage_basis", "entry_url"]
    for filename, records in (("candidates.csv", selected), ("audiobook-candidates.csv", audio)):
        with (directory / filename).open("w", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=fields, extrasaction="ignore")
            writer.writeheader()
            writer.writerows(records)
    # Preserve an inventory of primary evidence so the calculation can be audited.
    artifacts = [{"file": p.name, "bytes": p.stat().st_size, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(directory.iterdir()) if p.suffix in (".txt", ".html", ".xml") or p.name in {name for name, _ in inputs} | {"archive-bleak-house.json", "andersen-rating-check.http.json", "audio-download-links.json", "language-clicks.json", "language-counts.json", "description-evidence.json"}]
    save(directory / "evidence-inventory.json", artifacts)
    assert all(r["rating"] >= 4 and r["review_count"] >= 1 and r["page_resolved"] and r["free_book"] for r in audio)
    assert all(r["free_book"] and r["has_rating"] and r["review_count"] >= 1 for r in selected)
    assert not any(r["store_audiobook_file"] for r in rows)
    assert len({r["entry_url"] for r in selected}) == len(selected)
    assert not any(r["retain_audiobook"] for r in rows if not r["page_resolved"])
    assert len(random_rows) == 512
    print(json.dumps({key: summary[key] for key in ("scope", "candidates", "audiobook_rating_counts", "candidate_popularity_data", "candidate_descriptions", "hosted_audio_storage")}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--catalog-db", type=Path)
    args = parser.parse_args()
    main(args.directory, args.catalog_db)
