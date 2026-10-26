//! On-demand source ingestion: metadata goes to SQLite, EPUBs and covers to disk.
//! Audiobook downloads remain linked to their source.
use crate::{cover_from_image, curation, enrich, get, gutenberg, has_cover, links, manifest::SourceMetadata, source_cover, store_book};
use book_model::{agent_id_from_migration_seed, BookDetails, BookFormat, BookRecord, Contributor, LanguageTag, AUTHOR_MARC_RELATOR_CODE};
use reqwest::blocking::Client;
use reqwest::Url;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, from_value, to_string, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs::copy;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod crawl;

const SOURCE: &str = "loyalbooks.com";
static REQUEST_DELAY: AtomicU64 = AtomicU64::new(0);
static CRAWL_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);

fn request_delay(seconds: u64) {
    // Discovery remains enabled when a full-site run disables pacing.
    CRAWL_ACTIVE.store(true, Ordering::Relaxed);
    REQUEST_DELAY.store(seconds, Ordering::Relaxed);
    *LAST_REQUEST.lock().expect("request clock") = Some(Instant::now());
}

pub fn wait_for_request(url: &Url) {
    let delay = REQUEST_DELAY.load(Ordering::Relaxed);
    if delay == 0 || !matches!(url.host_str(), Some("loyalbooks.com" | "www.loyalbooks.com")) {
        return;
    }
    let mut last = LAST_REQUEST.lock().expect("request clock");
    if let Some(last) = *last {
        sleep(Duration::from_secs(delay).saturating_sub(last.elapsed()));
    }
    *last = Some(Instant::now());
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Popularity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
    pub scope: String,
    pub listing_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at_unix_seconds: Option<u64>,
    #[serde(flatten)]
    pub details: BTreeMap<String, Value>,
}

#[derive(Serialize)]
struct Evidence {
    title: String,
    authors: Vec<String>,
    languages: Vec<String>,
    free_book: bool,
    rating: f64,
    rating_count: u64,
    description: String,
    description_source_url: String,
    popularity: Vec<Popularity>,
    epub_url: Option<String>,
    advertised_epub_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    epub_import_error: Option<String>,
    audiobook_download_url: Option<String>,
    audiobook_delivery: Option<&'static str>,
    audiobook_duration_ms: Option<u64>,
    observed_at_unix_seconds: u64,
}

struct Page {
    id: String,
    url: Url,
    record: BookRecord,
    rating: f64,
    rating_count: u64,
    epub: Option<Url>,
    fallback_epub: Option<Url>,
    advertised_epub: Option<Url>,
    epub_import_error: Option<String>,
    audio: Option<Url>,
    audio_duration_ms: Option<u64>,
    cover: Option<Url>,
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("constant source selector")
}

fn text(element: ElementRef<'_>) -> String {
    element.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn property(html: &Html, css: &str) -> Option<String> {
    html.select(&selector(css)).next().map(|element| element.value().attr("content").map(str::to_owned).unwrap_or_else(|| text(element)))
}

fn source_url(value: &str, prefix: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") || !matches!(url.host_str(), Some("loyalbooks.com" | "www.loyalbooks.com")) || !url.path().starts_with(prefix) || !url.username().is_empty() || url.password().is_some() {
        return Err(format!("expected a Loyal Books {prefix} URL"));
    }
    Ok(url)
}

fn download_url(base: &Url, href: &str) -> Result<Url, String> {
    let url = base.join(href).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("source download URL must use HTTP or HTTPS without credentials".into());
    }
    canonical_archive_download(url)
}

pub(super) fn canonical_archive_download(mut url: Url) -> Result<Url, String> {
    if matches!(url.host_str(), Some("archive.org" | "www.archive.org")) && url.path().starts_with("/download/") {
        // Some source links contain /download//ITEM/FILE. Archive.org keeps
        // that empty segment when redirecting to storage, which returns 503.
        let path = format!("/{}", url.path().split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("/"));
        url.set_path(&path);
        url.set_scheme("https").map_err(|_| "invalid archive download scheme")?;
        url.set_host(Some("archive.org")).map_err(|e| e.to_string())?;
    }
    Ok(url)
}

/// A missing rating or an unrated/paid page is a rejection, not an import.
fn page(url: Url, raw: &str) -> Result<Option<Page>, String> {
    let html = Html::parse_document(raw);
    let page_title = property(&html, "title").unwrap_or_default();
    if !page_title.contains("Free at Loyal Books") {
        if !page_title.contains("Paid") && html.select(&selector("[itemtype$='/Book']")).next().is_none() {
            return Err("unrecognized source page; admission could not be checked".into());
        }
        return Ok(None);
    }
    let rating = property(&html, "[itemprop=aggregateRating] [itemprop=ratingValue]").and_then(|value| value.parse::<f64>().ok());
    let count = property(&html, "[itemprop=aggregateRating] [itemprop=reviewCount], [itemprop=aggregateRating] [itemprop=ratingCount]").and_then(|value| value.replace(',', "").parse::<u64>().ok());
    let (Some(rating), Some(rating_count)) = (rating, count) else { return Ok(None) };
    if !(1.0..=5.0).contains(&rating) || rating_count == 0 {
        return Ok(None);
    }
    let title = property(&html, "h1 [itemprop=name], h1").filter(|title| !title.is_empty()).ok_or("source page has no title")?;
    let mut record = BookRecord { metadata: BookDetails { title, ..Default::default() }, description: String::new() };
    let author = html
        .select(&selector("[itemprop=author]"))
        .find(|element| !element.ancestors().filter_map(ElementRef::wrap).any(|ancestor| ancestor.value().attr("itemprop") == Some("review") || ancestor.value().attr("itemtype").is_some_and(|value| value.ends_with("/Review"))));
    if let Some(name) = author.map(text).filter(|name| !name.is_empty()) {
        let id = agent_id_from_migration_seed(format!("{SOURCE}:author:{name}").as_bytes());
        record.contributors.push(Contributor::with_id(id, name, AUTHOR_MARC_RELATOR_CODE).map_err(|e| e.to_string())?);
    }
    if let Some(language) = property(&html, "[itemprop=inLanguage]") {
        record.book.languages.push(LanguageTag::parse(language_label(&language).unwrap_or(&language)).map_err(|e| e.to_string())?);
    } else if let Some(language) = title_language(&page_title) {
        record.book.languages.push(LanguageTag::parse(language).map_err(|e| e.to_string())?);
    }
    // Invalid paragraph nesting on source pages moves formatted text outside
    // the itemprop span during HTML parsing. The formatting class survives on
    // every repaired fragment, including the introductory text.
    let descriptions = html.select(&selector(".book-description")).map(text).filter(|value| !value.is_empty()).collect::<Vec<_>>();
    if !descriptions.is_empty() {
        record.description = descriptions.join("\n\n");
    } else if let Some(description) = html.select(&selector("[itemprop=description]")).next() {
        let paragraphs = description.select(&selector("p")).map(text).filter(|value| !value.is_empty()).collect::<Vec<_>>();
        record.description = if paragraphs.is_empty() { text(description) } else { paragraphs.join("\n\n") };
    }
    let mut epub = None;
    let mut fallback_epub = None;
    let mut audio = None;
    for link in html.select(&selector("a[href]")) {
        let href = link.value().attr("href").unwrap_or_default();
        if fallback_epub.is_none() && text(link).contains("Read eBook") {
            fallback_epub = gutenberg_epub(&url, href);
        }
        if epub.is_none() && (href.split('?').next().unwrap_or(href).ends_with(".epub") || href.contains("/download/epub/")) {
            epub = Some(download_url(&url, href)?);
        }
        if audio.is_none() && rating >= 4.0 && text(link).contains("MP3 Download") {
            audio = Some(download_url(&url, href)?);
        }
    }
    let cover = html.select(&selector("[itemprop=image][src]")).next().and_then(|element| element.value().attr("src")).map(|href| download_url(&url, href)).transpose()?;
    let audio_duration_ms = audio.as_ref().and_then(|_| audio_duration(&html));
    Ok(Some(Page { audio_duration_ms, id: url.path().trim_start_matches("/book/").to_owned(), url, record, rating, rating_count, advertised_epub: epub.clone(), epub, fallback_epub, epub_import_error: None, audio, cover }))
}

/// The source advertises the recording's total as `(HH:MM:SS long)`.
fn audio_duration(html: &Html) -> Option<u64> {
    let body = text(html.root_element());
    body.split(" long)").find_map(|prefix| {
        let clock = prefix.rsplit_once('(')?.1;
        let mut parts = clock.split(':');
        let hours = parts.next()?.parse::<u64>().ok()?;
        let minutes = parts.next()?.parse::<u64>().ok()?;
        let seconds = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() || minutes >= 60 || seconds >= 60 {
            return None;
        }
        hours.checked_mul(3600)?.checked_add(minutes * 60 + seconds)?.checked_mul(1000).filter(|duration| *duration > 0)
    })
}

fn gutenberg_epub(base: &Url, href: &str) -> Option<Url> {
    let url = download_url(base, href).ok()?;
    gutenberg::book_page(&url)
}

fn retain_epub(dir: &Path, tx: &mut Transaction<'_>, http: &Client, page: &Page, url: &Url) -> Result<(String, Url), String> {
    let gutenberg::Acquisition { epub: url, metadata } = gutenberg::resolve_acquisition(http, url)?;
    let mut record = page.record.clone();
    metadata.retain_evidence(&mut record);
    let bytes = get(http, &url)?;
    let name = url.path_segments().and_then(|mut segments| segments.next_back()).unwrap_or("book.epub");
    let mut savepoint = tx.savepoint().map_err(|e| e.to_string())?;
    match store_book(dir, &savepoint, http, &bytes, name, Vec::new(), page.cover.as_ref(), Some(&SourceMetadata::from_record(&record))) {
        Ok(hash) => {
            savepoint.commit().map_err(|e| e.to_string())?;
            Ok((hash, url))
        }
        Err(error) => {
            savepoint.rollback().map_err(|e| e.to_string())?;
            Err(error)
        }
    }
}

fn title_language(title: &str) -> Option<&'static str> {
    let language = title.strip_suffix(" - Free at Loyal Books")?.rsplit_once(" - ")?.1;
    // The source labels non-English recordings in the document title. An
    // absent label does not establish English; keep unknown languages unknown.
    language_label(language)
}

fn language_label(language: &str) -> Option<&'static str> {
    Some(match language {
        "Ancient Greek" => "grc",
        "Bulgarian" => "bg",
        "Chinese" => "zh",
        "Church Slavonic" => "cu",
        "Danish" => "da",
        "Dutch" => "nl",
        "English" => "en",
        "Esperanto" => "eo",
        "Finnish" => "fi",
        "French" => "fr",
        "German" => "de",
        "Greek" => "el",
        "Hebrew" => "he",
        "Hungarian" => "hu",
        "Italian" => "it",
        "Japanese" => "ja",
        "Javanese" => "jv",
        "Latin" => "la",
        "Latvian" => "lv",
        "Multilingual" => "mul",
        "Old English" => "ang",
        "Polish" => "pl",
        "Portuguese" => "pt",
        "Russian" => "ru",
        "Spanish" => "es",
        "Swedish" => "sv",
        "Tagalog" => "tl",
        "Tamil" => "ta",
        "Urdu" => "ur",
        "Cebuano" => "ceb",
        "Luo" => "luo",
        "Arabic" => "ar",
        "Indonesian" => "id",
        "Ukrainian" => "uk",
        "Korean" => "ko",
        "Telugu" => "te",
        "Croatian" => "hr",
        "Luxembourgish" => "lb",
        "Cantonese Chinese" => "yue",
        "Macedonian" => "mk",
        _ => return None,
    })
}

fn popularity(url: &Url, raw: &str, book_url: &Url) -> Result<Vec<Popularity>, String> {
    // Top_100 uses 100 entries per page. Language-featured positions are not
    // interchangeable with these site-wide ranks.
    let page_number = match url.path().trim_end_matches('/').strip_prefix("/Top_100") {
        Some("") => 1,
        Some(suffix) => suffix.strip_prefix('/').and_then(|value| value.parse::<usize>().ok()).filter(|number| *number > 0).ok_or("invalid popularity page number")?,
        None => return Err("popularity URL must name a Top_100 page".into()),
    };
    let offset = (page_number - 1).checked_mul(100).ok_or("popularity page number is too large")?;
    let html = Html::parse_document(raw);
    let mut seen = HashSet::new();
    for link in html.select(&selector("td.layout2-blue a[href]")) {
        let target = url.join(link.value().attr("href").unwrap_or_default()).map_err(|e| e.to_string())?;
        if target.path().starts_with("/book/") && seen.insert(target.path().to_owned()) && target.path() == book_url.path() {
            return Ok(vec![Popularity { rank: Some(offset + seen.len()), scope: "sitewide".into(), listing_url: url.to_string(), observed_at_unix_seconds: Some(now()), details: BTreeMap::new() }]);
        }
    }
    Ok(Vec::new())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn add(dir: &Path, db: &Connection, http: &Client, page_url: &str, listing_url: Option<&str>) -> Result<(), String> {
    add_with_epub(dir, db, http, page_url, listing_url, None)
}

pub fn add_with_epub(dir: &Path, db: &Connection, http: &Client, page_url: &str, listing_url: Option<&str>, epub_url: Option<&str>) -> Result<(), String> {
    let _timing = crate::timing::measure("source_ingest", page_url);
    let url = source_url(page_url, "/book/")?;
    let raw = String::from_utf8(get(http, &url)?).map_err(|e| e.to_string())?;
    ingest_page(dir, db, http, url, raw, listing_url, epub_url)
}

/// Consumes a prefetched source page; admission and every database mutation
/// still happen on the serialized writer. Prefetching never fetches audio.
pub(super) fn add_fetched(dir: &Path, db: &Connection, http: &Client, page_url: &str, raw: String) -> Result<(), String> {
    let _timing = crate::timing::measure("source_ingest", page_url);
    ingest_page(dir, db, http, source_url(page_url, "/book/")?, raw, None, None)
}

fn ingest_page(dir: &Path, db: &Connection, http: &Client, url: Url, raw: String, listing_url: Option<&str>, epub_url: Option<&str>) -> Result<(), String> {
    let page_url = url.as_str().to_owned();
    let parsing_timing = crate::timing::measure("source_parse_and_discovery", &page_url);
    if CRAWL_ACTIVE.load(Ordering::Relaxed) {
        crawl::discover_links(db, &url, &raw)?;
    }
    let Some(mut page) = page(url, &raw)? else {
        eprintln!("skipped: the source page is not a free book with a recorded rating");
        return Ok(());
    };
    drop(parsing_timing);
    if let Some(epub_url) = epub_url {
        if page.advertised_epub.is_none() {
            return Err("the source page does not offer an EPUB".into());
        }
        page.epub = Some(download_url(&page.url, epub_url)?);
    }
    if page.epub.is_none() && page.audio.is_none() {
        eprintln!("skipped: no eligible EPUB or audiobook link");
        return Ok(());
    }
    let popularity = if let Some(value) = listing_url {
        let url = source_url(value, "/Top_100")?;
        let raw = String::from_utf8(get(http, &url)?).map_err(|e| e.to_string())?;
        popularity(&url, &raw, &page.url)?
    } else {
        Vec::new()
    };
    let mut tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    // Only EPUB/source requests occur here; no audio URL is ever fetched.
    let hash = if let Some(url) = page.epub.clone() {
        let result = retain_epub(dir, &mut tx, http, &page, &url);
        let result = match result {
            Err(error) if !error.contains("enrichment failed") && page.fallback_epub.as_ref().is_some_and(|fallback| fallback != &url) => {
                let fallback = page.fallback_epub.clone().unwrap();
                eprintln!("source EPUB unavailable; trying its linked Gutenberg edition: {error}");
                match retain_epub(dir, &mut tx, http, &page, &fallback) {
                    Ok(result) => Ok(result),
                    Err(fallback_error) => Err(format!("{error}; linked EPUB also failed: {fallback_error}")),
                }
            }
            other => other,
        };
        let hash = match result {
            Ok((hash, url)) => {
                page.epub = Some(url);
                Some(hash)
            }
            Err(error) if page.audio.is_some() => {
                page.epub_import_error = Some(error);
                None
            }
            Err(error) => return Err(error),
        };
        if let Some(hash) = &hash {
            let has_other_source: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM book_source WHERE hash=?1 AND source<>?2)", params![hash, SOURCE], |row| row.get(0)).map_err(|e| e.to_string())?;
            if !has_other_source && !page.record.description.is_empty() {
                if let Some(mut record) = curation::record(&tx, &hash)? {
                    if record.description != page.record.description {
                        record.description = page.record.description.clone();
                        curation::set_record(&tx, &hash, &record)?;
                    }
                }
            }
            curation::add_source(&tx, hash, SOURCE, &page.id, Some(page.url.as_str()), page.epub.as_ref().map(Url::as_str))?;
        }
        hash
    } else {
        None
    };
    let enriched = if hash.is_none() && page.audio.is_some() {
        let key = links::entry_key(SOURCE, &page.id)?;
        let local_cover = source_cover(dir, &key, http, page.cover.as_ref());
        let (record, _, found) = enrich(http, page.record.clone(), &format!("{}.audiobook", page.id), !local_cover)?;
        Some((record, found))
    } else {
        None
    };
    let persist_timing = crate::timing::measure("source_persist", &page_url);
    persist(&tx, &page, hash.as_deref(), popularity, enriched)?;
    let external: Option<String> = tx.query_row("SELECT entry_key FROM external_book WHERE source=?1 AND entry_id=?2", params![SOURCE, page.id], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    drop(persist_timing);
    if let Some(entry) = external.filter(|entry| !has_cover(dir, entry)) {
        let _timing = crate::timing::measure("audio_covers", &page_url);
        // A cover can be retained even when the recording stays on its source.
        let reused = hash.as_ref().is_some_and(|hash| has_cover(dir, hash));
        let result = if reused {
            ["card", "detail"].iter().try_for_each(|size| copy(dir.join(format!("covers/{}-{size}.jpg", hash.as_ref().unwrap())), dir.join(format!("covers/{entry}-{size}.jpg"))).map(|_| ()).map_err(|e| e.to_string()))
        } else if let Some(cover) = &page.cover {
            get(http, cover).and_then(|bytes| cover_from_image(dir, &entry, &bytes))
        } else {
            Ok(())
        };
        if let Err(error) = result {
            eprintln!("optional audio cover unavailable for {}: {error}", page.id);
        }
    }
    eprintln!("ingested {} directly into the catalog", page.record.title);
    Ok(())
}

fn persist(db: &Connection, page: &Page, hash: Option<&str>, mut popularity: Vec<Popularity>, enriched_record: Option<(BookRecord, bool)>) -> Result<(), String> {
    let previous: Option<(String, Option<String>)> =
        db.query_row("SELECT metadata,book_hash FROM source_entry WHERE source=?1 AND entry_id=?2", params![SOURCE, page.id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(|e| e.to_string())?;
    let previous_metadata: Option<Value> = previous.as_ref().map(|(metadata, _)| from_str(metadata)).transpose().map_err(|e| e.to_string())?;
    let preserved_hash = if page.epub_import_error.is_some() && hash.is_none() { previous.as_ref().and_then(|(_, hash)| hash.as_deref()) } else { None };
    let hash = hash.or(preserved_hash);
    // A refresh without a listing must not erase an observed, dated rank.
    if let Some(previous) = &previous_metadata {
        if let Some(value) = previous.get("popularity") {
            let old: Vec<Popularity> = from_value(value.clone()).map_err(|e| e.to_string())?;
            for observation in old {
                if !popularity.iter().any(|new| new.listing_url == observation.listing_url) {
                    popularity.push(observation);
                }
            }
        }
    }
    let inspected = hash.map(|hash| curation::record(db, hash)).transpose()?.flatten().unwrap_or_else(|| page.record.clone());
    let (mut record, enriched) = enriched_record.unwrap_or((inspected, false));
    // A source can link a complete EPUB alongside a recording of one volume
    // or short story. Keep the audio rendition's own title and description.
    record.title = page.record.title.clone();
    record.subtitle = page.record.subtitle.clone();
    record.description = page.record.description.clone();
    let external_key = if let Some(audio) = &page.audio {
        Some(links::store_entry(
            db,
            &links::Entry {
                source: SOURCE.into(),
                entry_id: page.id.clone(),
                entry_url: page.url.to_string(),
                license: "source-declares-free".into(),
                record,
                file_url: Some(audio.to_string()),
                format: BookFormat::Mp3Folder,
                toc: Vec::new(),
                source_evidence: None,
                enriched,
            },
        )?)
    } else {
        // Refreshing a page that no longer qualifies must remove its old audio link.
        db.execute("DELETE FROM external_book WHERE source=?1 AND entry_id=?2", params![SOURCE, page.id]).map_err(|e| e.to_string())?;
        None
    };
    let evidence = Evidence {
        title: page.record.title.clone(),
        authors: page.record.authors().map(|author| author.name().to_owned()).collect(),
        languages: page.record.book.languages.iter().map(|language| language.as_str().to_owned()).collect(),
        free_book: true,
        rating: page.rating,
        rating_count: page.rating_count,
        description: page.record.description.clone(),
        description_source_url: page.url.to_string(),
        popularity,
        epub_url: if preserved_hash.is_some() { previous_metadata.as_ref().and_then(|value| value["epub_url"].as_str()).map(str::to_owned) } else { page.epub.as_ref().map(Url::to_string) },
        advertised_epub_url: page.advertised_epub.as_ref().map(Url::to_string),
        epub_import_error: page.epub_import_error.clone(),
        audiobook_download_url: page.audio.as_ref().map(Url::to_string),
        audiobook_delivery: page.audio.as_ref().map(|_| "external_download"),
        audiobook_duration_ms: page.audio_duration_ms,
        observed_at_unix_seconds: now(),
    };
    let metadata = to_string(&evidence).map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO source_entry(source,entry_id,entry_url,book_hash,external_entry_key,metadata) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(source,entry_id) DO UPDATE SET entry_url=excluded.entry_url,book_hash=excluded.book_hash,external_entry_key=excluded.external_entry_key,metadata=excluded.metadata",
        params![SOURCE, page.id, page.url.as_str(), hash, external_key, metadata],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::read_dir;

    fn html(rating: &str, count: u64) -> String {
        format!(
            r#"<title>Book - Free at Loyal Books</title><h1>Book</h1><a itemprop="author">Author</a><span itemprop="description"><p>First paragraph.</p><p>Second &amp; paragraph.</p></span><span itemprop="aggregateRating"><span itemprop="ratingValue">{rating}</span><span itemprop="reviewCount">{count}</span></span><a href="/download/epub/book.epub">ePUB eBook</a><a href="https://archive.org/download/recording/audio.zip">MP3 Download</a>"#
        )
    }

    #[test]
    fn admission_keeps_rated_epubs_and_only_four_plus_audio() {
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        assert!(page(url.clone(), &html("", 1)).unwrap().is_none());
        assert!(page(url.clone(), &html("5", 0)).unwrap().is_none());
        assert!(page(url.clone(), &html("5", 1).replace("Free at Loyal Books", "Paid book")).unwrap().is_none());
        assert!(source_url("https://www.loyalbooks.com/paid/book", "/book/").is_err());
        let low = page(url.clone(), &html("3", 1)).unwrap().unwrap();
        assert!(low.epub.is_some() && low.audio.is_none());
        let high = page(url, &html("4", 1)).unwrap().unwrap();
        assert!(high.epub.is_some() && high.audio.is_some());
        assert_eq!(high.record.description, "First paragraph.\n\nSecond & paragraph.");
    }

    #[test]
    fn archive_audio_links_remove_empty_segments_before_storage_redirects() {
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let raw = html("5", 1).replace("https://archive.org/download/recording/audio.zip", "http://www.archive.org/download//recording/audio.zip");
        let parsed = page(url.clone(), &raw).unwrap().unwrap();
        assert_eq!(parsed.audio.unwrap().as_str(), "https://archive.org/download/recording/audio.zip");
        assert_eq!(download_url(&url, "http://www.archive.org/download//recording/a%20b.zip?download=1").unwrap().as_str(), "https://archive.org/download/recording/a%20b.zip?download=1");
        // Empty segments can be significant on unrelated providers.
        assert_eq!(download_url(&url, "https://example.org/download//audio.zip").unwrap().as_str(), "https://example.org/download//audio.zip");
        assert!(download_url(&url, "https://user:password@archive.org/download//recording/audio.zip").is_err());
    }

    #[test]
    fn direct_database_write_preserves_metadata_and_is_idempotent_without_audio_files() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let page = page(url.clone(), &html("5", 2)).unwrap().unwrap();
        let listing = source_url("https://www.loyalbooks.com/Top_100/2", "/Top_100").unwrap();
        let ranks = popularity(&listing, r#"<table><tr><td class="layout2-blue"><a href="/book/other">Other</a><a href="/book/other">Other</a></td><td class="layout2-blue"><a href="/book/book">Book</a></td></tr></table>"#, &url).unwrap();
        assert_eq!(ranks[0].rank, Some(102));
        let tx = db.unchecked_transaction().unwrap();
        persist(&tx, &page, None, ranks.clone(), None).unwrap();
        tx.commit().unwrap();
        persist(&db, &page, None, Vec::new(), None).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM external_book", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(db.query_row("SELECT count(*) FROM source_entry", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        let (metadata, record, link): (String, String, String) =
            db.query_row("SELECT source_entry.metadata,external_book.record,external_book.file_url FROM source_entry JOIN external_book ON entry_key=external_entry_key", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        let evidence: Value = from_str(&metadata).unwrap();
        assert_eq!(evidence["rating"], 5.0);
        assert_eq!(evidence["popularity"][0]["rank"], 102);
        assert_eq!(evidence["description_source_url"], url.as_str());
        assert_eq!(from_str::<BookRecord>(&record).unwrap().description, page.record.description);
        assert_eq!(link, "https://archive.org/download/recording/audio.zip");
        assert_eq!(read_dir(dir.path().join("books")).unwrap().count(), 0);
        assert_eq!(read_dir(dir.path()).unwrap().count(), 4); // DB plus empty books/covers/inbox.
    }

    #[test]
    fn advertised_audio_duration_is_parsed_and_retained_as_source_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let raw = format!("{}<span>(07:43:09 long)</span>", html("5", 2));
        let page = page(url, &raw).unwrap().unwrap();
        assert_eq!(page.audio_duration_ms, Some(27_789_000));
        persist(&db, &page, None, Vec::new(), None).unwrap();
        let metadata: String = db.query_row("SELECT metadata FROM source_entry", [], |row| row.get(0)).unwrap();
        assert_eq!(from_str::<Value>(&metadata).unwrap()["audiobook_duration_ms"], 27_789_000);
        for clock in ["00:00:00", "01:60:00", "01:00:60", "1:2", "1:2:3:4", "18446744073709551615:00:00"] {
            assert_eq!(audio_duration(&Html::parse_document(&format!("({clock} long)"))), None, "{clock}");
        }
        assert_eq!(audio_duration(&Html::parse_document("no running time")), None);
    }

    #[test]
    fn reviewer_names_do_not_become_book_authors() {
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let raw = html("5", 2).replace("<a itemprop=\"author\">Author</a>", "<div itemprop=\"review\"><span itemprop=\"author\">Reviewer</span></div>");
        assert!(page(url, &raw).unwrap().unwrap().record.contributors.is_empty());
    }

    #[test]
    fn failed_epub_refresh_preserves_verified_epub_and_audio_with_failure_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let mut page = page(url, &html("5", 2)).unwrap().unwrap();
        let hash = "4".repeat(64);
        curation::set_record(&db, &hash, &page.record).unwrap();
        persist(&db, &page, Some(&hash), Vec::new(), None).unwrap();
        page.epub_import_error = Some("truncated EPUB download".into());
        persist(&db, &page, None, Vec::new(), None).unwrap();
        let (stored, external, metadata): (String, String, String) = db.query_row("SELECT book_hash,external_entry_key,metadata FROM source_entry", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(stored, hash);
        assert!(!external.is_empty());
        let evidence: Value = from_str(&metadata).unwrap();
        assert_eq!(evidence["epub_import_error"], "truncated EPUB download");
        assert_eq!(evidence["epub_url"], page.advertised_epub.unwrap().as_str());
        assert_eq!(read_dir(dir.path().join("books")).unwrap().count(), 0);
    }

    #[test]
    fn audio_keeps_its_volume_title_when_the_linked_epub_is_the_complete_work() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let url = source_url("https://www.loyalbooks.com/book/wealth-of-nations-4", "/book/").unwrap();
        let mut page = page(url, &html("5", 2)).unwrap().unwrap();
        page.record.title = "The Wealth of Nations, Book 4".into();
        page.record.subtitle = Some("A recording of the fourth book".into());
        let mut complete = page.record.clone();
        complete.title = "An Inquiry into the Nature and Causes of the Wealth of Nations".into();
        complete.subtitle = None;
        complete.description = "The complete five-book work".into();
        let hash = "5".repeat(64);
        curation::set_record(&db, &hash, &complete).unwrap();
        persist(&db, &page, Some(&hash), Vec::new(), None).unwrap();
        let json: String = db.query_row("SELECT record FROM external_book", [], |row| row.get(0)).unwrap();
        let audio: BookRecord = from_str(&json).unwrap();
        assert_eq!(audio.title, page.record.title);
        assert_eq!(audio.subtitle, page.record.subtitle);
        assert_eq!(audio.description, page.record.description);
        assert_eq!(curation::record(&db, &hash).unwrap().unwrap(), complete);
        // Metadata-only enrichment must preserve the same source-rendition fields.
        persist(&db, &page, None, Vec::new(), Some((complete, true))).unwrap();
        let title: String = db.query_row("SELECT title FROM external_book", [], |row| row.get(0)).unwrap();
        assert_eq!(title, page.record.title);
    }

    #[test]
    fn unknown_source_responses_are_retryable_and_large_vote_counts_are_accepted() {
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        assert!(page(url.clone(), "<title>Gateway error</title>").is_err());
        let raw = html("5", 1234).replace(">1234<", ">1,234<");
        assert_eq!(page(url, &raw).unwrap().unwrap().rating_count, 1234);
        for label in ["Cebuano", "Luo", "Arabic", "Indonesian", "Ukrainian", "Korean", "Telugu", "Croatian", "Luxembourgish", "Cantonese Chinese", "Macedonian"] {
            assert!(LanguageTag::parse(language_label(label).unwrap()).is_ok());
        }
    }

    #[test]
    fn epub_fallback_uses_only_the_gutenberg_edition_linked_from_the_source() {
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        assert_eq!(gutenberg_epub(&url, "http://www.gutenberg.org/dirs/5/9/2/5921/5921-h/5921-h.htm").unwrap().as_str(), "https://www.gutenberg.org/ebooks/5921");
        assert_eq!(gutenberg_epub(&url, "https://www.gutenberg.org/ebooks/5921").unwrap().as_str(), "https://www.gutenberg.org/ebooks/5921");
        assert!(gutenberg_epub(&url, "https://foreign.test/ebooks/5921").is_none());
        assert!(gutenberg_epub(&url, "https://www.gutenberg.org/ebooks/").is_none());
        assert!(gutenberg_epub(&url, "javascript:alert(1)").is_none());
    }

    #[test]
    fn language_popularity_observations_survive_a_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let page = page(url, &html("5", 2)).unwrap().unwrap();
        let evidence: Value = from_str(r#"[{"scope":"language_featured","language":"Swedish","position":2,"listing_url":"https://www.loyalbooks.com/language/Swedish","observed_on":"2026-10-12"}]"#).unwrap();
        let observations = from_value(evidence.clone()).unwrap();
        persist(&db, &page, None, observations, None).unwrap();
        persist(&db, &page, None, Vec::new(), None).unwrap();
        let metadata: String = db.query_row("SELECT metadata FROM source_entry", [], |row| row.get(0)).unwrap();
        assert_eq!(from_str::<Value>(&metadata).unwrap()["popularity"], evidence);
    }

    #[test]
    fn malformed_description_keeps_the_intro_and_all_paragraphs() {
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let raw = html("5", 2).replace(
            "<span itemprop=\"description\"><p>First paragraph.</p><p>Second &amp; paragraph.</p></span>",
            "<p><span itemprop=\"description\"><font class=\"book-description\">Intro text.<p>First paragraph.<p>Second &amp; paragraph.</font></span>",
        );
        let description = page(url, &raw).unwrap().unwrap().record.description;
        assert!(description.contains("Intro text."));
        assert!(description.contains("First paragraph."));
        assert!(description.contains("Second & paragraph."));
    }

    #[test]
    fn source_language_labels_are_preserved_without_guessing_english() {
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let raw = html("5", 2).replace("Book - Free at Loyal Books", "Book - Swedish - Free at Loyal Books");
        let labeled = page(url.clone(), &raw).unwrap().unwrap();
        assert_eq!(labeled.record.book.languages[0].as_str(), "sv");
        assert!(page(url, &html("5", 2)).unwrap().unwrap().record.book.languages.is_empty());
    }

    #[test]
    fn direct_metadata_write_rolls_back_with_its_caller_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let url = source_url("https://www.loyalbooks.com/book/book", "/book/").unwrap();
        let page = page(url, &html("5", 2)).unwrap().unwrap();
        let tx = db.unchecked_transaction().unwrap();
        persist(&tx, &page, None, Vec::new(), None).unwrap();
        tx.rollback().unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM external_book", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(db.query_row("SELECT count(*) FROM source_entry", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
}
