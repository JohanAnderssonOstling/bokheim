//! Apply a human-reviewed, content-hash-bound classification manifest.
//! Defaults to a transaction rollback; pass --apply after inspecting the result.
use book_model::BookSubject;
use content_address::ContentHash;

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

#[derive(Deserialize)]
struct Correction {
    content_hash: ContentHash,
    title: String,
    lcc: String,
    source: String,
    evidence: String,
}

fn identifiers(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<Vec<Vec<String>>> {
    conn.prepare(include_str!("sql/apply_reviewed_subjects/identifiers_select.sql"))?
        .query_map([hash.as_str()], |row| (0..5).map(|i| row.get(i)).collect())?.collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if !(args.len() == 3 || args.len() == 4 && args[3] == "--apply") {
        return Err("usage: apply_reviewed_subjects DATABASE MANIFEST [--apply]".into());
    }
    let corrections: Vec<Correction> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let mut conn = Connection::open_with_flags(&args[1], OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    client_library::schema::configure_connection(&conn)?;
    let tx = client_library::schema::library_write_transaction(&mut conn)?;
    for item in corrections {
        let title: String = tx.query_row(include_str!("sql/apply_reviewed_subjects/main_select.sql"), [item.content_hash.as_str()], |r| r.get(0))?;
        if title != item.title { return Err(format!("Title changed: {}", item.content_hash).into()); }
        if !item.source.starts_with("reviewed:") || item.evidence.is_empty() { return Err("review evidence is required".into()); }
        let paths = subject_projection::unified_subject_paths("lcc", &item.lcc);
        if paths.is_empty() { return Err(format!("No unified mapping for {}", item.lcc).into()); }
        let conflicts: i64 = tx.query_row(include_str!("sql/apply_reviewed_subjects/main_select_2.sql"), [item.content_hash.as_str()], |r| r.get(0))?;
        if conflicts != 0 { return Err(format!("Existing metadata needs review: {title}").into()); }
        let before = identifiers(&tx, &item.content_hash)?;
        let subject = BookSubject::new(None, &item.lcc, &item.source, Some("lcc".into()), Some(item.lcc.clone()))?;
        let added = client_library::external_metadata::apply_rich_enrichment_in_transaction(&tx, item.content_hash, "reviewed:historia:20260907", "", None, &[], &[], vec![subject], "updated", Some(&item.evidence), false)?;
        if added != 1 || identifiers(&tx, &item.content_hash)? != before { return Err(format!("Verification failed: {title}").into()); }
        let assigned: i64 = tx.query_row(include_str!("sql/apply_reviewed_subjects/main_select_3.sql"), [item.content_hash.as_str()], |r| r.get(0))?;
        if assigned == 0 { return Err(format!("Subject projection failed: {title}").into()); }
        println!("{title}: {} -> {} unified assignment(s)", item.lcc, assigned);
    }
    // Use the normal Subjects read path to refresh its lazily rebuilt projections.
    client_library::browse::subject_facet_counts(&tx, subject_projection::ROOT_SUBJECT_PATH, "", &[], &[])?;
    if args.len() == 4 { tx.commit()?; println!("Committed reviewed corrections."); }
    else { tx.rollback()?; println!("Dry run passed; rolled back all corrections."); }
    Ok(())
}
