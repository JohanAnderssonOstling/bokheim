//! Resolve CSV classification probes against an explicitly selected taxonomy.
//! This audits the production matcher without using its bundled taxonomy image.
use std::io;
use std::path::Path;
use subject_projection::{canonical_lcc_notation, read_unified_taxonomy_sqlite, UnifiedTaxonomy, LCC_SYSTEM_ID};

fn ids(values: &[i64]) -> String {
    values.iter().map(i64::to_string).collect::<Vec<_>>().join("|")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let path = args.get(1).ok_or("usage: literature_coverage TAXONOMY.sqlite3 < codes.csv")?;
    let taxonomy = UnifiedTaxonomy::from_concepts(read_unified_taxonomy_sqlite(Path::new(path))?)?;
    eprintln!("taxonomy revision: {}", taxonomy.revision_id());
    let mut input = csv::Reader::from_reader(io::stdin());
    let mut output = csv::Writer::from_writer(io::stdout());
    output.write_record(["code", "canonical", "candidate_ids", "resolved_ids", "exact_selector", "subclass_ids", "unresolved_fragments"])?;
    for row in input.records() {
        let row = row?;
        let code = row.get(0).ok_or("missing code column")?;
        let canonical = canonical_lcc_notation(code).unwrap_or_default();
        let letters = canonical.chars().take_while(char::is_ascii_alphabetic).collect::<String>();
        let report = taxonomy.lcc_match_report(code);
        output.write_record([
            code,
            &canonical,
            &ids(&taxonomy.match_candidates(LCC_SYSTEM_ID, code)),
            &ids(&taxonomy.matching_concept_ids(LCC_SYSTEM_ID, code)),
            if taxonomy.has_exact_source_code(LCC_SYSTEM_ID, code) { "true" } else { "false" },
            &ids(&taxonomy.matching_concept_ids(LCC_SYSTEM_ID, &letters)),
            &format!("{:?}", report.unresolved_fragments),
        ])?;
    }
    output.flush()?;
    Ok(())
}
