//! Stream unresolved LCC classifications, optionally comparing an external outline.
#[path = "../lcc_outline.rs"]
mod lcc_outline;
use lcc_outline::LccOutline;
use std::{collections::BTreeMap, path::Path};
use subject_projection::{canonical_lcc_notation, read_unified_taxonomy_sqlite, UnifiedTaxonomy, LCC_SYSTEM_ID};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let taxonomy = UnifiedTaxonomy::from_concepts(read_unified_taxonomy_sqlite(Path::new(args.get(1).ok_or("usage: lcc_gaps TAXONOMY.sqlite3 CODE_USAGE.sqlite3 [LCC_OUTLINE.csv]")?))?)?;
    let db = rusqlite::Connection::open_with_flags(args.get(2).ok_or("missing code usage database")?, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let outline = args.get(3).map(|path| LccOutline::read(Path::new(path))).transpose()?;
    let mut query = db.prepare("SELECT notation,uses FROM code_usage WHERE scheme=2 ORDER BY notation")?;
    let mut output = csv::Writer::from_writer(std::io::stdout());
    output.write_record(["status", "code", "uses", "canonical", "outline_path", "concept_ids"])?;
    let mut counts = BTreeMap::<&str, (u64, i64)>::new();
    for row in query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
        let (code, uses) = row?;
        let ids = taxonomy.match_candidates(LCC_SYSTEM_ID, &code);
        let assigned = ids.len() == 1 || (ids.len() > 1 && !taxonomy.matching_concept_ids(LCC_SYSTEM_ID, &code).is_empty());
        let canonical = (!assigned).then(|| canonical_lcc_notation(&code)).flatten();
        let status = match ids.len() {
            _ if assigned => "matched",
            0 if canonical.is_none() => "parser_rejected",
            0 => "unmatched",
            _ => "ambiguous",
        };
        let count = counts.entry(status).or_default();
        count.0 += 1;
        count.1 += uses;
        if assigned {
            continue;
        }
        let outline_path = canonical.as_deref().and_then(|code| outline.as_ref()?.subject_path(code)).unwrap_or_default();
        output.write_record([status, &code, &uses.to_string(), canonical.as_deref().unwrap_or_default(), &outline_path, &ids.iter().map(i64::to_string).collect::<Vec<_>>().join("|")])?;
    }
    output.flush()?;
    for (status, (strings, uses)) in counts {
        eprintln!("{status}\t{strings}\t{uses}");
    }
    Ok(())
}
