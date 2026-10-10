//! Audit exact-selector conflicts and optional SQLite code_usage classifications.
use std::path::Path;
use subject_projection::{read_unified_taxonomy_sqlite, UnifiedTaxonomy, LCC_SYSTEM_ID};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let taxonomy = UnifiedTaxonomy::from_concepts(read_unified_taxonomy_sqlite(Path::new(args.get(1).ok_or("usage: lcc_coverage TAXONOMY.sqlite3 [CODE_USAGE.sqlite3]")?))?)?;
    let conflicts = taxonomy.lcc_selector_conflicts();
    eprintln!("{} conflicting exact LCC selectors", conflicts.len());
    let mut output = csv::Writer::from_writer(std::io::stdout());
    output.write_record(["kind", "code", "concept_ids"])?;
    for (code, ids) in conflicts {
        output.write_record(["selector_conflict", &code, &ids.iter().map(i64::to_string).collect::<Vec<_>>().join("|")])?;
    }
    if let Some(path) = args.get(2) {
        let db = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut query = db.prepare("SELECT notation FROM code_usage WHERE scheme=2 ORDER BY notation")?;
        for row in query.query_map([], |row| row.get::<_, String>(0))? {
            let code = row?;
            let ids = taxonomy.match_candidates(LCC_SYSTEM_ID, &code);
            let kind = match ids.len() {
                0 => "unmatched",
                1 => "matched",
                _ if !taxonomy.matching_concept_ids(LCC_SYSTEM_ID, &code).is_empty() => "matched",
                _ => "ambiguous",
            };
            output.write_record([kind, &code, &ids.iter().map(i64::to_string).collect::<Vec<_>>().join("|")])?;
        }
    }
    output.flush()?;
    Ok(())
}
