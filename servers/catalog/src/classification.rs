//! Audit classification specificity without changing bibliographic records.
use book_model::{BookRecord, BookSubject};
use rusqlite::Connection;
use serde_json::json;
use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use subject_projection::{has_lcc_classification, is_specific_lcc_code, project_subject_codes, project_subject_concepts, reviewed_source_heading_subject, unified_taxonomy_routes};

pub fn audit(db: &Connection) -> Result<(), String> {
    let mut rows = Vec::new();
    let routes = unified_taxonomy_routes();
    for (table, key) in [("book", "hash"), ("external_book", "entry_key")] {
        let mut statement = db.prepare(&format!("SELECT {key},record FROM {table} ORDER BY {key}")).map_err(|error| error.to_string())?;
        let records = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|error| error.to_string())?;
        for row in records {
            let (key, raw) = row.map_err(|error| error.to_string())?;
            let record: BookRecord = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
            let lcc = record.book.subjects.iter().filter(|subject| subject.authority().is_some_and(|authority| authority.eq_ignore_ascii_case("lcc"))).filter_map(|subject| subject.code()).collect::<Vec<_>>();
            let specific = lcc.iter().copied().filter(|code| is_specific_lcc_code(code)).collect::<Vec<_>>();
            let codes = project_subject_codes(&record.book.subjects);
            let concepts = project_subject_concepts(&codes).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>();
            let paths = routes.iter().filter(|route| concepts.contains(&route.concept_id())).map(|route| route.path()).collect::<Vec<_>>();
            rows.push(json!({"table":table,"key":key,"title":record.title,"needs_refinement":!lcc.is_empty() && specific.is_empty(),"specific_lcc_codes":specific,"projected_paths":paths}));
        }
    }
    println!("{}", json!(rows));
    Ok(())
}

/// A persistent local validator lets backfills reject unsupported class numbers
/// using exactly the same taxonomy as catalog navigation, without guessing.
pub fn resolve_codes() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let codes: Vec<String> = serde_json::from_str(&line.map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        let rows = codes.iter().map(|code| json!({"code":code,"specific":is_specific_lcc_code(code)})).collect::<Vec<_>>();
        writeln!(stdout, "{}", json!(rows)).and_then(|_| stdout.flush()).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Materialize only the shared, approved raw-heading rules, without lookup or
/// catalog writes. One JSON subject array per line makes backfills repeatable.
pub fn recover_headings() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let subjects: Vec<BookSubject> = serde_json::from_str(&line.map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        let mut additions = Vec::new();
        if !has_lcc_classification(&subjects) {
            for subject in subjects.iter().filter_map(reviewed_source_heading_subject) {
                if !subjects.contains(&subject) && !additions.contains(&subject) {
                    additions.push(subject);
                }
            }
        }
        let all_subjects = subjects.iter().chain(&additions).cloned().collect::<Vec<_>>();
        let projected = project_subject_codes(&all_subjects);
        let offset = subjects.len();
        additions = additions.into_iter().enumerate().filter(|(position, _)| projected.iter().any(|code| code.evidence_positions().contains(&(offset + position)))).map(|(_, subject)| subject).collect();
        writeln!(stdout, "{}", json!(additions)).and_then(|_| stdout.flush()).map_err(|error| error.to_string())?;
    }
    Ok(())
}
