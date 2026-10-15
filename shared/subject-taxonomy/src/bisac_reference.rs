//! Compile the BISAC reference into indexed SQLite rows at build time.
use crate::compact_whitespace;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::Path;

pub fn split_path(value: &str) -> Vec<String> {
    value.replace("::", "/").replace("--", "/").replace(['>', '|', '›', '→'], "/").split('/').map(compact_whitespace).filter(|component| !component.is_empty()).collect()
}

pub fn normalize_term(value: &str) -> String {
    compact_whitespace(value).to_lowercase()
}

pub fn normalize_path(value: &str) -> String {
    split_path(value).iter().map(|component| normalize_term(component)).collect::<Vec<_>>().join("/")
}

pub fn compile(path: &Path, csv: &str, revision: &str) -> rusqlite::Result<()> {
    let mut nodes = BTreeMap::<String, (Option<String>, Vec<String>)>::new();
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(csv.as_bytes());
    for record in reader.records() {
        let record = record.expect("bundled BISAC CSV must be valid");
        let code = record.get(0).unwrap_or_default().trim();
        let description = record.get(1).unwrap_or_default().trim();
        if code.is_empty() || description.is_empty() {
            continue;
        }
        let components = description.split('/').map(compact_whitespace).filter(|component| !component.is_empty()).collect::<Vec<_>>();
        for index in 0..components.len() {
            let path_components = &components[..=index];
            let path = path_components.join(" / ");
            let final_code = (index + 1 == components.len()).then(|| code.to_owned());
            nodes
                .entry(path)
                .and_modify(|node| {
                    if final_code.is_some() {
                        node.0 = final_code.clone();
                    }
                })
                .or_insert_with(|| (final_code, path_components.iter().map(|component| normalize_term(component)).collect()));
        }
    }
    let mut db = Connection::open(path)?;
    db.execute_batch(
        "PRAGMA journal_mode=OFF;
        DROP TABLE IF EXISTS bisac_component; DROP TABLE IF EXISTS bisac_node; DROP TABLE IF EXISTS reference_meta;
        CREATE TABLE reference_meta(revision TEXT NOT NULL);
        CREATE TABLE bisac_node(id INTEGER PRIMARY KEY,path TEXT NOT NULL,code TEXT,normalized_path TEXT NOT NULL,tail_component TEXT NOT NULL,component_count INTEGER NOT NULL);
        CREATE INDEX bisac_path ON bisac_node(normalized_path,id DESC);
        CREATE INDEX bisac_code ON bisac_node(code,id DESC) WHERE code IS NOT NULL;
        CREATE INDEX bisac_tail ON bisac_node(tail_component,id);",
    )?;
    let tx = db.transaction()?;
    tx.execute("INSERT INTO reference_meta VALUES(?1)", [revision])?;
    // BTreeMap assigns IDs in binary path order. DESC IDs retain the existing
    // path tie-breaking rules without repeating each path inside the indexes.
    for (index, (path, (code, components))) in nodes.into_iter().enumerate() {
        let id = index as i64 + 1;
        tx.execute("INSERT INTO bisac_node VALUES(?1,?2,?3,?4,?5,?6)", params![id, path, code, normalize_path(&path), components.last().expect("BISAC path has components"), components.len() as i64])?;
    }
    tx.commit()?;
    db.execute_batch("VACUUM;")
}
