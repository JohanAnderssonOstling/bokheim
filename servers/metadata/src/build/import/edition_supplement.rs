use super::core::import_input_identity;
use super::pipeline::{read_json_resumable_mapped, JsonBatchSizes};
use super::sidecar::{SidecarBuilder, SidecarSpec};
use crate::build::edition_descriptions::{index_path, VERSION};
use crate::build::work_edition_index::identity;
use crate::*;

#[derive(Deserialize)]
struct DescriptionEditionRecord {
    key: String,
    #[serde(default, deserialize_with = "deserialize_optional_description")]
    description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_key_references")]
    works: Vec<KeyReference>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    isbn_10: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_array")]
    isbn_13: Vec<String>,
}

struct DescriptionEdition {
    id: i64,
    description: String,
    work: Option<i64>,
    isbns: BTreeSet<Isbn13>,
}

/// Add descriptions to an existing immutable catalog without copying it.
pub fn import_edition_descriptions(editions_path: &Path, source: &Path, record_limit: Option<usize>) -> Result<serde_json::Value, MetadataError> {
    let started = Instant::now();
    let source = source.canonicalize().map_err(error)?;
    let before = identity(&source)?;
    let imported = crate::build::read_connection(&source)?;
    crate::build::validate_source(&imported)?;
    let dump_date: String = imported.query_row("SELECT dump_date FROM snapshot WHERE singleton=1", [], |r| r.get(0)).map_err(error)?;
    if crate::dump_date(editions_path).is_some_and(|date| date != dump_date) {
        return Err(MetadataError("edition dump date does not match the imported catalog".into()));
    }
    let input = import_input_identity(editions_path)?;
    let pinned: Option<(i64, i64)> = imported.query_row("SELECT size_bytes,modified_ms FROM import_input WHERE name='editions'", [], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(error)?;
    if pinned.is_none_or(|pinned| pinned != (input.1, input.2)) {
        return Err(MetadataError("edition dump size or modification time does not match the catalog's pinned import input".into()));
    }
    drop(imported);
    let output = index_path(&source);
    let mut builder = SidecarBuilder::open(editions_path, &output, &dump_date, record_limit, SidecarSpec::edition_descriptions())?;
    let c = builder.connection_mut();
    let saved: Option<(i64, i64, String)> = c.query_row("SELECT version,source_bytes,source_modified_ns FROM index_source WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(error)?;
    if let Some(saved) = saved {
        if saved != (VERSION, before.0, before.1.clone()) {
            return Err(MetadataError("cannot resume edition descriptions: imported catalog changed".into()));
        }
    } else {
        c.execute("INSERT INTO index_source VALUES(1,?1,?2,?3)", params![VERSION, before.0, before.1]).map_err(error)?;
    }
    let records = read_json_resumable_mapped(
        c,
        editions_path,
        record_limit,
        "edition_descriptions",
        JsonBatchSizes { checkpoint: OPEN_LIBRARY_CHECKPOINT_RECORDS, parse: PIPELINE_PARSE_RECORDS },
        &|record: DescriptionEditionRecord| {
            let id = open_library_id(&record.key, 'M').filter(|id| *id > 0)?;
            let description = valid_open_library_description(record.description.as_deref()?)?;
            let works = record.works.iter().filter_map(|work| open_library_id(&work.key, 'W')).filter(|id| *id > 0).collect::<BTreeSet<_>>();
            // Multiple work links do not establish a single work identity.
            let work = (works.len() == 1).then(|| *works.first().expect("one work"));
            let isbns = record.isbn_10.iter().chain(&record.isbn_13).filter_map(|isbn| Isbn13::parse(isbn)).collect();
            Some(DescriptionEdition { id, description, work, isbns })
        },
        &mut |transaction, records| {
            let mut description = transaction.prepare_cached("INSERT INTO edition_description VALUES(?1,?2)").map_err(error)?;
            let mut work = transaction.prepare_cached("INSERT INTO edition_description_work VALUES(?1,?2)").map_err(error)?;
            let mut isbn = transaction.prepare_cached("INSERT INTO edition_description_isbn VALUES(?1,?2)").map_err(error)?;
            for record in records {
                description.execute(params![record.id, record.description]).map_err(error)?;
                if let Some(work_id) = record.work {
                    work.execute(params![work_id, record.id]).map_err(error)?;
                }
                for isbn13 in record.isbns {
                    isbn.execute(params![isbn13, record.id]).map_err(error)?;
                }
            }
            Ok(())
        },
    )?;
    if identity(&source)? != before || import_input_identity(editions_path)? != input {
        return Err(MetadataError("catalog or edition dump changed while importing edition descriptions".into()));
    }
    let count: i64 = builder.connection_mut().query_row("SELECT count(*) FROM edition_description", [], |r| r.get(0)).map_err(error)?;
    builder.finalize_once("edition_descriptions_finalize", |_| Ok(()))?;
    builder.publish()?;
    Ok(serde_json::json!({"output":output,"edition_records":records,"descriptions":count,"bytes":fs::metadata(output).map_err(error)?.len(),"elapsed_seconds":started.elapsed().as_secs_f64()}))
}
