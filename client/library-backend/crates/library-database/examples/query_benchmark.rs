//! Measures real local query APIs on disposable snapshots, never live databases.
//! Usage: query_benchmark SNAPSHOT_DIRECTORY
use library_database::{Database, DatabaseError};
use library_model::{BrowseLocation, BrowsePageRequest, LibraryBrowseQuery, NotebookQuery, NotebookRequest};
use serde_json::json;
use std::{error::Error, hint::black_box, path::Path, time::Instant};
use sync_common::ROOT_DIR_ID;

fn query(db: &Database, kind: &str, search: &str) -> Result<(usize, usize), DatabaseError> {
    let request = NotebookRequest { page: BrowsePageRequest { section_limit: 0, book_limit: 8, ..Default::default() }, ..Default::default() };
    let notebook = NotebookQuery { search: search.to_owned(), ..Default::default() };
    match kind {
        "notebook" => Ok(db.notebook(&notebook, &request, false)?.map_or((0, 0), |page| (page.page.book_total, page.books.len()))),
        "full_text" => {
            let Some(page) = db.text_search(&notebook, &request)? else { return Ok((0, 0)) };
            let revision = page.page.result_revision.as_deref().unwrap_or_default();
            for entry in &page.books {
                if let Some(book) = entry.book.library_file() {
                    black_box(db.text_search_matches(&notebook, revision, book, 0, 8)?);
                }
            }
            Ok((page.page.book_total, page.books.len()))
        }
        _ => {
            let location = if kind == "folder" { BrowseLocation::Folder(ROOT_DIR_ID) } else { BrowseLocation::Books };
            let query = LibraryBrowseQuery { location, search: search.to_owned(), file_types: vec![], languages: vec![], chip_sort: Default::default(), book_sort: Default::default(), hide_finished: false };
            let page = db.browse_page(&query, false, BrowsePageRequest::default())?;
            Ok((page.page.as_ref().map_or(0, |state| state.book_total), page.contents.books.len()))
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    client_platform_runtime::text_search::set_enabled(true);
    let directory = std::env::args_os().nth(1).ok_or("snapshot directory required")?;
    let root = Path::new(&directory);
    let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("libraries.json"))?)?;
    for library in metadata.as_array().ok_or("library list required")? {
        let path = Path::new(library["snapshot"].as_str().ok_or("snapshot path required")?);
        // Only operate on files directly inside this explicitly supplied snapshot directory.
        if path.parent() != Some(root) {
            return Err("snapshot outside benchmark directory".into());
        }
        for kind in ["books", "folder", "notebook", "full_text"] {
            let searches: &[&str] =
                if kind == "full_text" { &["the", "och", "hist", "history", "dem", "democracy", "social", "zzzxqnevermatch"] } else { &["", "h", "hi", "his", "hist", "history", "d", "de", "dem", "democracy", "social", "zzzxqnevermatch"] };
            for &search in searches {
                let mut fresh = Vec::new();
                let mut cached = Vec::new();
                let mut rows = (0, 0);
                for _ in 0..5 {
                    // No initialization/migration; production preparation runs against the snapshot as-is.
                    let db = Database::open(path)?;
                    let started = Instant::now();
                    rows = black_box(query(&db, kind, search)?);
                    fresh.push(started.elapsed().as_secs_f64() * 1000.0);
                    let started = Instant::now();
                    black_box(query(&db, kind, search)?);
                    cached.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                println!("{}", json!({"library":library["id"],"kind":kind,"query":search,"matching_books":rows.0,"page_books":rows.1,"fresh_ms":fresh,"cached_ms":cached}));
            }
        }
    }
    Ok(())
}
