//! A durable, single-worker sitemap acquisition. HTML stays in memory.
use super::{add, add_fetched, now as unix_time, request_delay, selector, source_url, Popularity, SOURCE};
use crate::{get, publish};
use reqwest::{blocking::Client, Url};
use rusqlite::{params, Connection, OptionalExtension};
use scraper::Html;
use serde_json::{from_str, json, to_string, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::Command;
use std::thread::{sleep, spawn, JoinHandle};
use std::time::Duration;

fn now() -> i64 {
    unix_time() as i64
}

const SITEMAP: &str = "https://www.loyalbooks.com/sitemap.xml";
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS source_crawl_run(
 source TEXT PRIMARY KEY,status TEXT NOT NULL,started_at INTEGER NOT NULL,
 sitemap_count INTEGER NOT NULL DEFAULT 0,delay_seconds INTEGER NOT NULL DEFAULT 60,
 published_entries INTEGER NOT NULL DEFAULT 0,published_at INTEGER NOT NULL DEFAULT 0,
 updated_at INTEGER NOT NULL,detail TEXT
) STRICT;
CREATE TABLE IF NOT EXISTS source_crawl_page(
 source TEXT NOT NULL,entry_id TEXT NOT NULL,url TEXT NOT NULL,kind TEXT NOT NULL,
 ordinal INTEGER NOT NULL,state TEXT NOT NULL DEFAULT 'pending',attempts INTEGER NOT NULL DEFAULT 0,
 next_attempt INTEGER NOT NULL DEFAULT 0,checked_at INTEGER,message TEXT,
 PRIMARY KEY(source,entry_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS source_crawl_ready ON source_crawl_page(source,ordinal);
CREATE TABLE IF NOT EXISTS source_crawl_popularity(
 source TEXT NOT NULL,entry_id TEXT NOT NULL,listing_url TEXT NOT NULL,metadata TEXT NOT NULL,
 PRIMARY KEY(source,entry_id,listing_url)
) STRICT, WITHOUT ROWID;
";

fn setup(db: &Connection) -> Result<(), String> {
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())
}

fn update_run(db: &Connection, state: &str, detail: Option<&str>) -> Result<(), String> {
    db.execute("UPDATE source_crawl_run SET status=?2,updated_at=?3,detail=?4 WHERE source=?1", params![SOURCE, state, now(), detail]).map_err(|e| e.to_string())?;
    Ok(())
}

fn sitemap(raw: &str) -> Result<Vec<Url>, String> {
    let xml = roxmltree::Document::parse(raw).map_err(|e| e.to_string())?;
    if xml.root_element().tag_name().name() != "urlset" {
        return Err("expected a source sitemap URL set".into());
    }
    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    for entry in xml.root_element().children().filter(|node| node.is_element()) {
        let Some(value) = entry.children().find(|node| node.is_element() && node.tag_name().name() == "loc").and_then(|node| node.text()) else { continue };
        let mut url = source_url(value.trim(), "/")?;
        url.set_scheme("https").map_err(|_| "invalid source scheme")?;
        url.set_host(Some("www.loyalbooks.com")).map_err(|e| e.to_string())?;
        url.set_fragment(None);
        if url.path().starts_with("/book/carousel/") || url.query().is_some() {
            continue;
        }
        if (url.path().starts_with("/book/") || url.path().starts_with("/Top_100") || url.path().starts_with("/language/")) && seen.insert(url.path().to_owned()) {
            urls.push(url);
        }
    }
    if !urls.iter().any(|url| url.path().starts_with("/book/")) {
        return Err("sitemap contains no book pages".into());
    }
    Ok(urls)
}

fn seed(db: &Connection, urls: &[Url]) -> Result<(), String> {
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut books = 0;
    let mut languages = 0;
    for url in urls {
        let (id, kind, ordinal) = if let Some(id) = url.path().strip_prefix("/book/") {
            let ordinal = books * 2 + 1;
            books += 1;
            (id.to_owned(), "book", ordinal)
        } else if url.path().starts_with("/language/") {
            languages += 1;
            (url.path().to_owned(), "listing", languages * 200 + 2)
        } else {
            let page = url.path().trim_end_matches('/').rsplit('/').next().and_then(|value| value.parse::<i64>().ok()).unwrap_or(1);
            (url.path().to_owned(), "listing", (page - 1) * 200)
        };
        tx.execute("INSERT INTO source_crawl_page(source,entry_id,url,kind,ordinal) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(source,entry_id) DO UPDATE SET url=excluded.url", params![SOURCE, id, url.as_str(), kind, ordinal])
            .map_err(|e| e.to_string())?;
    }
    // Entries acquired in this session already passed live admission checks.
    tx.execute("UPDATE source_crawl_page SET state='imported',checked_at=?2 WHERE source=?1 AND state='pending' AND kind='book' AND EXISTS(SELECT 1 FROM source_entry e WHERE e.source=source_crawl_page.source AND e.entry_id=source_crawl_page.entry_id AND (e.book_hash IS NOT NULL OR e.external_entry_key IS NOT NULL) AND json_extract(e.metadata,'$.epub_import_error') IS NULL)", params![SOURCE, now()]).map_err(|e| e.to_string())?;
    tx.execute("UPDATE source_crawl_run SET sitemap_count=?2,updated_at=?3 WHERE source=?1", params![SOURCE, books, now()]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn observations(url: &Url, raw: &str) -> Result<Vec<(String, Popularity)>, String> {
    let language = url.path().strip_prefix("/language/").map(|value| value.replace('_', " "));
    let page = if language.is_none() { url.path().trim_end_matches('/').rsplit('/').next().and_then(|value| value.parse::<usize>().ok()).unwrap_or(1) } else { 1 };
    let html = Html::parse_document(raw);
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    for anchor in html.select(&selector("td.layout2-blue a[href]")) {
        let target = url.join(anchor.value().attr("href").unwrap_or_default()).map_err(|e| e.to_string())?;
        if !matches!(target.host_str(), Some("loyalbooks.com" | "www.loyalbooks.com")) {
            continue;
        }
        let Some(id) = target.path().strip_prefix("/book/") else { continue };
        if !seen.insert(id.to_owned()) {
            continue;
        }
        let mut details = BTreeMap::new();
        let (scope, rank) = if let Some(language) = &language {
            details.insert("language".into(), json!(language));
            details.insert("position".into(), json!(seen.len()));
            ("language_featured", None)
        } else {
            ("sitewide", Some((page - 1) * 100 + seen.len()))
        };
        results.push((id.to_owned(), Popularity { rank, scope: scope.into(), listing_url: url.to_string(), observed_at_unix_seconds: Some(unix_time()), details }));
        if language.is_some() && results.len() == 2 {
            break;
        }
    }
    if results.is_empty() {
        return Err("listing contains no recognizable book positions".into());
    }
    Ok(results)
}

pub(super) fn discover_links(db: &Connection, url: &Url, raw: &str) -> Result<(), String> {
    let html = Html::parse_document(raw);
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut ordinal: i64 = tx.query_row("SELECT coalesce(max(ordinal),0)+1 FROM source_crawl_page WHERE source=?1", [SOURCE], |row| row.get(0)).map_err(|e| e.to_string())?;
    for anchor in html.select(&selector("a[href]")) {
        let Ok(mut target) = url.join(anchor.value().attr("href").unwrap_or_default()) else { continue };
        if !matches!(target.scheme(), "http" | "https") || !matches!(target.host_str(), Some("loyalbooks.com" | "www.loyalbooks.com")) || target.query().is_some() || !target.username().is_empty() || target.password().is_some() {
            continue;
        }
        let kind = if target.path().starts_with("/book/") && target.path() != "/book/" && !target.path().starts_with("/book/carousel/") {
            "book"
        } else if target.path() == "/Top_100" || target.path().strip_prefix("/Top_100/").is_some_and(|number| number.parse::<usize>().is_ok_and(|number| number > 0)) || target.path().starts_with("/language/") {
            "listing"
        } else {
            continue;
        };
        target.set_scheme("https").map_err(|_| "invalid source scheme")?;
        target.set_host(Some("www.loyalbooks.com")).map_err(|e| e.to_string())?;
        target.set_fragment(None);
        let id = if kind == "book" { target.path().trim_start_matches("/book/") } else { target.path() };
        let state: &str = if kind == "book"
            && tx
                .query_row("SELECT EXISTS(SELECT 1 FROM source_entry WHERE source=?1 AND entry_id=?2 AND json_extract(metadata,'$.epub_import_error') IS NULL)", params![SOURCE, id], |row| row.get::<_, bool>(0))
                .map_err(|e| e.to_string())?
        {
            "imported"
        } else {
            "pending"
        };
        tx.execute("INSERT OR IGNORE INTO source_crawl_page(source,entry_id,url,kind,ordinal,state) VALUES(?1,?2,?3,?4,?5,?6)", params![SOURCE, id, target.as_str(), kind, ordinal, state]).map_err(|e| e.to_string())?;
        ordinal += 1;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn apply_observations(db: &Connection, id: &str) -> Result<(), String> {
    let previous: Option<String> = db.query_row("SELECT metadata FROM source_entry WHERE source=?1 AND entry_id=?2", params![SOURCE, id], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
    let Some(previous) = previous else { return Ok(()) };
    let mut metadata: Value = from_str(&previous).map_err(|e| e.to_string())?;
    let mut observations = metadata["popularity"].as_array().cloned().unwrap_or_default();
    let mut statement = db.prepare("SELECT metadata FROM source_crawl_popularity WHERE source=?1 AND entry_id=?2 ORDER BY listing_url").map_err(|e| e.to_string())?;
    for row in statement.query_map(params![SOURCE, id], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())? {
        let observation: Value = from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        observations.retain(|old| old["listing_url"] != observation["listing_url"]);
        observations.push(observation);
    }
    metadata["popularity"] = json!(observations);
    db.execute("UPDATE source_entry SET metadata=?3 WHERE source=?1 AND entry_id=?2", params![SOURCE, id, to_string(&metadata).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    Ok(())
}

struct Prefetch {
    url: String,
    worker: JoinHandle<Result<String, String>>,
}

impl Prefetch {
    fn start(http: &Client, url: &str) -> Self {
        let client = http.clone();
        let target = url.to_owned();
        let worker = spawn(move || {
            let url = Url::parse(&target).map_err(|e| e.to_string())?;
            String::from_utf8(get(&client, &url)?).map_err(|e| e.to_string())
        });
        Self { url: url.to_owned(), worker }
    }

    fn finish(self, url: &str) -> Option<Result<String, String>> {
        // Join even when discovery changed the next task, keeping at most one
        // background fetch alive. No queue state advances until acquisition.
        let result = self.worker.join().unwrap_or_else(|_| Err("source prefetch worker panicked".into()));
        (self.url == url).then_some(result)
    }
}

fn acquire(db: &Connection, dir: &Path, http: &Client, id: &str, kind: &str, url: &str, fetched: Option<Result<String, String>>) -> Result<&'static str, String> {
    if kind == "listing" {
        let url = source_url(url, "/")?;
        let raw = match fetched {
            Some(result) => result?,
            None => String::from_utf8(get(http, &url)?).map_err(|e| e.to_string())?,
        };
        let observations = observations(&url, &raw)?;
        discover_links(db, &url, &raw)?;
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        for (id, observation) in observations {
            tx.execute(
                "INSERT INTO source_crawl_popularity VALUES(?1,?2,?3,?4) ON CONFLICT(source,entry_id,listing_url) DO UPDATE SET metadata=excluded.metadata",
                params![SOURCE, id, observation.listing_url, to_string(&observation).map_err(|e| e.to_string())?],
            )
            .map_err(|e| e.to_string())?;
            apply_observations(&tx, &id)?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        return Ok("observed");
    }
    match fetched {
        Some(result) => add_fetched(dir, db, http, url, result?)?,
        None => add(dir, db, http, url, None)?,
    }
    apply_observations(db, id)?;
    let error: Option<Option<String>> = db.query_row("SELECT json_extract(metadata,'$.epub_import_error') FROM source_entry WHERE source=?1 AND entry_id=?2", params![SOURCE, id], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
    Ok(match error {
        Some(Some(_)) => "partial",
        Some(None) => "imported",
        None => "skipped",
    })
}

fn available_bytes(dir: &Path) -> Result<u64, String> {
    let output = Command::new("df").arg("-Pk").arg(dir).output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("could not check available storage".into());
    }
    let raw = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let fields = raw.lines().last().ok_or("empty storage report")?.split_whitespace().collect::<Vec<_>>();
    fields.get(fields.len().saturating_sub(3)).and_then(|value| value.parse::<u64>().ok()).and_then(|value| value.checked_mul(1024)).ok_or("invalid storage report".into())
}

fn publish_local(db: &Connection, dir: &Path) -> Result<(), String> {
    let out = dir.join("published");
    let releases = out.join("releases");
    fs::create_dir_all(&releases).map_err(|e| e.to_string())?;
    let release = fs::read_dir(&releases).map_err(|e| e.to_string())?.filter_map(Result::ok).filter_map(|entry| entry.file_name().to_str().and_then(|value| value.parse::<u64>().ok())).max().unwrap_or(0) + 1;
    publish::publish(dir, db, &out, release)?;
    let snapshot = releases.join(format!("{release}/catalog.sqlite3"));
    let verified = Connection::open(&snapshot).map_err(|e| e.to_string())?;
    let integrity: String = verified.query_row("PRAGMA integrity_check", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if integrity != "ok" || verified.prepare("PRAGMA foreign_key_check").map_err(|e| e.to_string())?.exists([]).map_err(|e| e.to_string())? {
        return Err("snapshot integrity check failed".into());
    }
    let installed = dir.parent().ok_or("catalog has no parent directory")?.join("catalog.sqlite3");
    let temporary = installed.with_extension("sqlite3.crawl-partial");
    fs::copy(&snapshot, &temporary).and_then(|_| fs::rename(&temporary, &installed)).map_err(|e| e.to_string())?;
    let count: i64 = db.query_row("SELECT count(*) FROM source_entry WHERE source=?1", [SOURCE], |row| row.get(0)).map_err(|e| e.to_string())?;
    db.execute("UPDATE source_crawl_run SET published_entries=?2,published_at=?3 WHERE source=?1", params![SOURCE, count, now()]).map_err(|e| e.to_string())?;
    eprintln!("Installed local catalog release {release}");
    Ok(())
}

pub fn run(dir: &Path, db: &Connection, http: &Client, retry: bool) -> Result<(), String> {
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("loyalbooks-crawl.lock")).map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|e| format!("another Loyal Books crawler holds the lock: {e}"))?;
    setup(db)?;
    db.busy_timeout(Duration::from_secs(30)).map_err(|e| e.to_string())?;
    db.execute("INSERT INTO source_crawl_run(source,status,started_at,updated_at,published_entries) VALUES(?1,'discovering',?2,?2,(SELECT count(*) FROM source_entry WHERE source=?1)) ON CONFLICT(source) DO UPDATE SET status='discovering',updated_at=excluded.updated_at", params![SOURCE, now()]).map_err(|e| e.to_string())?;
    if retry {
        db.execute("UPDATE source_crawl_page SET state='pending',attempts=0,next_attempt=0 WHERE source=?1 AND state IN ('failed','partial')", [SOURCE]).map_err(|e| e.to_string())?;
    }
    let configured_delay = std::env::var("BOKHEIM_LOYALBOOKS_REQUEST_DELAY_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>().ok().filter(|seconds| *seconds <= i64::MAX as u64).ok_or("request delay must be a non-negative integer number of seconds"))
        .transpose()?;
    request_delay(configured_delay.unwrap_or(60));
    // Refresh discovery on every restart. Finished pages remain finished.
    let robots_url = Url::parse("https://www.loyalbooks.com/robots.txt").map_err(|e| e.to_string())?;
    let robots = String::from_utf8(get(http, &robots_url)?).map_err(|e| e.to_string())?;
    let requested_delay = robots
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.trim().eq_ignore_ascii_case("crawl-delay"))
        .filter_map(|(_, value)| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.ceil() as u64)
        .max()
        .unwrap_or(60)
        .max(60);
    let delay = configured_delay.unwrap_or(requested_delay);
    eprintln!("Loyal Books request interval: {delay}s; source requested: {requested_delay}s");
    if robots.lines().any(|line| line.split_once(':').is_some_and(|(key, value)| key.trim().eq_ignore_ascii_case("disallow") && matches!(value.trim(), "/" | "/book/" | "/book"))) {
        return Err("source robots rules disallow book acquisition".into());
    }
    request_delay(delay);
    db.execute("UPDATE source_crawl_run SET delay_seconds=?2 WHERE source=?1", params![SOURCE, delay as i64]).map_err(|e| e.to_string())?;
    let raw = String::from_utf8(get(http, &Url::parse(SITEMAP).map_err(|e| e.to_string())?)?).map_err(|e| e.to_string())?;
    seed(db, &sitemap(&raw)?)?;
    update_run(db, "running", None)?;
    let mut prefetch: Option<Prefetch> = None;
    loop {
        if available_bytes(dir)? < 512 * 1024 * 1024 {
            update_run(db, "waiting_for_space", Some("less than 512 MiB free; will resume automatically when storage is available"))?;
            eprintln!("Waiting for available storage");
            sleep(Duration::from_secs(600));
            continue;
        }
        let mut statement =
            db.prepare("SELECT entry_id,kind,url FROM source_crawl_page WHERE source=?1 AND state IN ('pending','failed','partial') AND attempts<3 AND next_attempt<=?2 ORDER BY ordinal LIMIT 2").map_err(|e| e.to_string())?;
        let tasks =
            statement.query_map(params![SOURCE, now()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        drop(statement);
        let mut tasks = tasks.into_iter();
        let task = tasks.next();
        let Some((id, kind, url)) = task else {
            let next: Option<i64> = db.query_row("SELECT min(next_attempt) FROM source_crawl_page WHERE source=?1 AND state IN ('pending','failed','partial') AND attempts<3", [SOURCE], |row| row.get(0)).map_err(|e| e.to_string())?;
            if let Some(next) = next {
                sleep(Duration::from_secs(next.saturating_sub(now()).clamp(1, 60) as u64));
                continue;
            }
            publish_local(db, dir)?;
            let failures: i64 = db.query_row("SELECT count(*) FROM source_crawl_page WHERE source=?1 AND state IN ('failed','partial')", [SOURCE], |row| row.get(0)).map_err(|e| e.to_string())?;
            update_run(db, if failures == 0 { "completed" } else { "completed_with_errors" }, None)?;
            return status(db);
        };
        update_run(db, "running", Some(&url))?;
        let fetched = prefetch.take().and_then(|task| task.finish(&url));
        // With explicit zero pacing, overlap one source-page GET with the
        // current book's enrichment, EPUB/cover processing and SQLite writes.
        // Other pacing configurations keep their original sequential behavior.
        if delay == 0 {
            if let Some((_, _, next_url)) = tasks.next() {
                prefetch = Some(Prefetch::start(http, &next_url));
            }
        }
        let _timing = crate::timing::measure("crawl_acquire", &url);
        let result = acquire(db, dir, http, &id, &kind, &url, fetched);
        drop(_timing);
        let (state, message) = match result {
            Ok(state) => (state, None),
            Err(error) => ("failed", Some(error)),
        };
        let next_attempt = if matches!(state, "failed" | "partial") { now() + 3600 } else { 0 };
        db.execute("UPDATE source_crawl_page SET state=?3,attempts=attempts+1,checked_at=?4,next_attempt=?5,message=?6 WHERE source=?1 AND entry_id=?2", params![SOURCE, id, state, now(), next_attempt, message])
            .map_err(|e| e.to_string())?;
        eprintln!("{state}: {url}{}", message.as_ref().map(|error| format!(": {error}")).unwrap_or_default());
        if message.as_ref().is_some_and(|error| error.contains("429") || error.contains("503") || error.contains("timed out")) {
            update_run(db, "waiting_for_source", message.as_deref())?;
            sleep(Duration::from_secs(900));
        }
        let (count, published, at): (i64, i64, i64) = db
            .query_row("SELECT (SELECT count(*) FROM source_entry WHERE source=?1),published_entries,published_at FROM source_crawl_run WHERE source=?1", [SOURCE], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| e.to_string())?;
        if (at == 0 && count > published) || count - published >= 100 || (at != 0 && now().saturating_sub(at) >= 86400) {
            if let Err(error) = publish_local(db, dir) {
                update_run(db, "publication_failed", Some(&error))?;
                return Err(error);
            }
        }
    }
}

pub fn status(db: &Connection) -> Result<(), String> {
    setup(db)?;
    let run: Option<Value> = db.query_row("SELECT status,sitemap_count,delay_seconds,started_at,updated_at,detail,published_entries,published_at FROM source_crawl_run WHERE source=?1", [SOURCE], |row| Ok(json!({"status":row.get::<_, String>(0)?,"sitemap_book_pages":row.get::<_, i64>(1)?,"delay_seconds":row.get::<_, i64>(2)?,"started_at":row.get::<_, i64>(3)?,"updated_at":row.get::<_, i64>(4)?,"detail":row.get::<_, Option<String>>(5)?,"published_entries":row.get::<_, i64>(6)?,"published_at":row.get::<_, i64>(7)?}))).optional().map_err(|e| e.to_string())?;
    let mut counts = BTreeMap::new();
    let mut statement = db.prepare("SELECT kind,state,count(*) FROM source_crawl_page WHERE source=?1 GROUP BY kind,state").map_err(|e| e.to_string())?;
    for row in statement.query_map([SOURCE], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))).map_err(|e| e.to_string())? {
        let (kind, state, count) = row.map_err(|e| e.to_string())?;
        counts.insert(format!("{kind}_{state}"), count);
    }
    let discovered: i64 = counts.iter().filter(|(key, _)| key.starts_with("book_")).map(|(_, count)| count).sum();
    println!("{}", serde_json::to_string_pretty(&json!({"source":SOURCE,"run":run,"discovered_book_pages":discovered,"pages":counts})).map_err(|e| e.to_string())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curation;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn prefetch_reuses_the_response_without_advancing_the_queue_or_bypassing_admission() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/book/not-rated", server.local_addr().unwrap());
        let (sent, received) = mpsc::channel();
        let body = "<title>Unrated - Free at Loyal Books</title><h1>Unrated</h1><a href='https://example.test/audio.zip'>MP3 Download</a>";
        let serving = spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert!(String::from_utf8(request).unwrap().starts_with("GET /book/not-rated "));
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            sent.send(()).unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        setup(&db).unwrap();
        let loyal_url = "https://www.loyalbooks.com/book/not-rated";
        seed(&db, &[Url::parse(loyal_url).unwrap()]).unwrap();
        let http = Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
        let prefetched = Prefetch::start(&http, &url);
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        let queue: (String, i64) = db.query_row("SELECT state,attempts FROM source_crawl_page", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(queue, ("pending".into(), 0));
        let fetched = prefetched.finish(&url);
        // The server has closed. Acquisition succeeds using the fetched HTML,
        // while the unrated book and its audio link are still excluded.
        serving.join().unwrap();
        assert_eq!(acquire(&db, dir.path(), &http, "not-rated", "book", loyal_url, fetched).unwrap(), "skipped");
        assert_eq!(db.query_row("SELECT count(*) FROM source_entry", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(fs::read_dir(dir.path().join("books")).unwrap().count(), 0);
    }

    #[test]
    fn discovery_is_deduplicated_and_resumable_without_dropping_failures() {
        let urls = sitemap(r#"<urlset><url><loc>http://loyalbooks.com/book/a</loc></url><url><loc>https://www.loyalbooks.com/book/a</loc></url><url><loc>https://www.loyalbooks.com/book/carousel/1</loc></url><url><loc>https://www.loyalbooks.com/Top_100/2</loc></url></urlset>"#).unwrap();
        assert_eq!(urls.len(), 2);
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        setup(&db).unwrap();
        seed(&db, &urls).unwrap();
        db.execute("UPDATE source_crawl_page SET state='failed',attempts=2 WHERE kind='book'", []).unwrap();
        seed(&db, &urls).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM source_crawl_page", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(db.query_row("SELECT attempts FROM source_crawl_page WHERE kind='book'", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    }

    #[test]
    fn popularity_keeps_language_positions_separate_and_refreshes_known_entries() {
        let raw = r#"<table><tr><td class="layout2-blue"><a href="/book/a">A</a><a href="/book/a">A</a><a href="/book/b">B</a><a href="/book/c">C</a></td></tr></table>"#;
        let site = observations(&Url::parse("https://www.loyalbooks.com/Top_100/2").unwrap(), raw).unwrap();
        assert_eq!(site[0].1.rank, Some(101));
        assert_eq!(site[2].1.rank, Some(103));
        let language = observations(&Url::parse("https://www.loyalbooks.com/language/Old_English").unwrap(), raw).unwrap();
        assert_eq!(language.len(), 2);
        assert_eq!(language[0].1.rank, None);
        assert_eq!(language[1].1.details["position"], 2);
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        setup(&db).unwrap();
        db.execute("INSERT INTO source_entry(source,entry_id,entry_url,metadata) VALUES(?1,'a','https://www.loyalbooks.com/book/a','{\"rating\":5,\"popularity\":[]}')", [SOURCE]).unwrap();
        for (_, observation) in [site[0].clone(), language[0].clone()] {
            db.execute("INSERT INTO source_crawl_popularity VALUES(?1,'a',?2,?3)", params![SOURCE, observation.listing_url, to_string(&observation).unwrap()]).unwrap();
        }
        apply_observations(&db, "a").unwrap();
        apply_observations(&db, "a").unwrap();
        let metadata: String = db.query_row("SELECT metadata FROM source_entry", [], |row| row.get(0)).unwrap();
        let value: Value = from_str(&metadata).unwrap();
        assert_eq!(value["popularity"].as_array().unwrap().len(), 2);
        assert_eq!(value["rating"], 5);
    }

    #[test]
    fn linked_books_expand_discovery_but_search_and_foreign_links_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        setup(&db).unwrap();
        let url = Url::parse("https://www.loyalbooks.com/book/a").unwrap();
        let raw = r#"<a href="/book/new">New</a><a href="/book/new#details">Duplicate</a><a href="/Top_100/500">New listing</a><a href="/search/new">Search</a><a href="/book/carousel/1">Carousel</a><a href="https://foreign.test/book/b">Foreign</a>"#;
        discover_links(&db, &url, raw).unwrap();
        discover_links(&db, &url, raw).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM source_crawl_page", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(db.query_row("SELECT url FROM source_crawl_page WHERE kind='book'", [], |row| row.get::<_, String>(0)).unwrap(), "https://www.loyalbooks.com/book/new");
    }
}
