use super::super::{
    error, io, params, BTreeSet, BufRead, BufReader, Child, ChildStdout, Command, Connection, File, HashSet, MetadataError, MultiBzDecoder, MultiGzDecoder, OptionalExtension, Path, Read, Stdio, WikidataClaim, WikidataClaimValue,
    WikidataRecord, OL_PERSON_KIND, OL_PLACE_KIND, WIKIDATA_CHECKPOINT_RECORDS, WIKIDATA_PIPELINE_PARSE_RECORDS,
};
use super::core::{completed_phase, mark_materialized, normalize_entity_heading};
use super::pipeline::{read_json_resumable_mapped, DumpFormat};

const WIKIDATA_FINALIZE_SQL: &str = include_str!("wikidata_finalize.sql");

pub(crate) fn import_wikidata(connection: &mut Connection, path: &Path, limit: Option<usize>) -> Result<usize, MetadataError> {
    let place_targets = evidence_targets(connection, OL_PLACE_KIND)?;
    let person_targets = evidence_targets(connection, OL_PERSON_KIND)?;
    let extract = |record| extract_wikidata_entity(record, &place_targets, &person_targets);
    let count =
        read_json_resumable_mapped::<WikidataRecord, WikidataExtract>(connection, path, limit, "wikidata_scan", WIKIDATA_CHECKPOINT_RECORDS, WIKIDATA_PIPELINE_PARSE_RECORDS, DumpFormat::Wikidata, &extract, &mut insert_wikidata_batch)?;
    materialize_wikidata(connection)?;
    finalize_wikidata(connection, count)?;
    println!("read {count} Wikidata entities; resolved unique exact headings conservatively");
    Ok(count)
}

fn insert_wikidata_batch(transaction: &rusqlite::Transaction<'_>, records: Vec<WikidataExtract>) -> Result<(), MetadataError> {
    let mut entity_insert = transaction.prepare_cached("INSERT INTO entity_stage(wikidata_id,kind,label,birth_year,death_year,fictional,latitude_e7,longitude_e7) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)").map_err(error)?;
    let mut alias_insert = transaction.prepare_cached("INSERT INTO entity_alias_stage(wikidata_id,alias) VALUES(?1,?2)").map_err(error)?;
    let mut parent_insert = transaction.prepare_cached("INSERT INTO place_parent_stage(place_id,parent_id) VALUES(?1,?2)").map_err(error)?;
    let mut country_insert = transaction.prepare_cached("INSERT INTO place_country_stage(place_id,country_id) VALUES(?1,?2)").map_err(error)?;
    let mut candidate_insert = transaction.prepare_cached("INSERT INTO wikidata_candidate_stage(kind,normalized_name,wikidata_id) VALUES(?1,?2,?3)").map_err(error)?;
    let mut place_candidate_insert = transaction.prepare_cached("INSERT INTO wikidata_place_candidate_stage(normalized_name,wikidata_id) VALUES(?1,?2)").map_err(error)?;
    let mut instance_insert = transaction.prepare_cached("INSERT INTO wikidata_instance_stage(wikidata_id,class_id) VALUES(?1,?2)").map_err(error)?;
    let mut subclass_insert = transaction.prepare_cached("INSERT INTO wikidata_subclass_stage(child_id,parent_id) VALUES(?1,?2)").map_err(error)?;
    for extracted in records {
        let qid = extracted.qid;
        if let Some(entity) = extracted.entity {
            entity_insert.execute(params![qid, entity.kind, entity.label, entity.birth_year, entity.death_year, i64::from(entity.fictional), entity.latitude_e7, entity.longitude_e7]).map_err(error)?;
        }
        for alias in extracted.aliases {
            alias_insert.execute(params![qid, alias]).map_err(error)?;
        }
        for parent in extracted.parents {
            parent_insert.execute(params![qid, parent]).map_err(error)?;
        }
        for country in extracted.countries {
            country_insert.execute(params![qid, country]).map_err(error)?;
        }
        for (kind, name) in extracted.candidates {
            candidate_insert.execute(params![kind, name, qid]).map_err(error)?;
        }
        for name in extracted.place_candidates {
            place_candidate_insert.execute(params![name, qid]).map_err(error)?;
        }
        for class_id in extracted.instances {
            instance_insert.execute(params![qid, class_id]).map_err(error)?;
        }
        for parent in extracted.subclasses {
            subclass_insert.execute(params![qid, parent]).map_err(error)?;
        }
    }
    Ok(())
}

fn finalize_wikidata(connection: &mut Connection, count: usize) -> Result<(), MetadataError> {
    if completed_phase(connection, "wikidata_finalize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction.execute_batch(WIKIDATA_FINALIZE_SQL).map_err(error)?;
    transaction
        .execute("INSERT INTO import_checkpoint(phase,records_read,completed) VALUES('wikidata_finalize',?1,1) ON CONFLICT(phase) DO UPDATE SET records_read=excluded.records_read,completed=1", [i64::try_from(count).map_err(error)?])
        .map_err(error)?;
    transaction.commit().map_err(error)
}

fn materialize_wikidata(connection: &mut Connection) -> Result<(), MetadataError> {
    if completed_phase(connection, "wikidata_materialize")?.is_some() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(error)?;
    transaction
        .execute_batch(
            "INSERT OR REPLACE INTO entity(wikidata_id,kind,label,birth_year,death_year,fictional,latitude_e7,longitude_e7)
                 SELECT wikidata_id,kind,label,birth_year,death_year,fictional,latitude_e7,longitude_e7 FROM entity_stage ORDER BY wikidata_id;
             INSERT OR IGNORE INTO entity_alias(wikidata_id,alias)
                 SELECT wikidata_id,alias FROM entity_alias_stage ORDER BY wikidata_id,alias;
             INSERT OR IGNORE INTO place_parent(place_id,parent_id)
                 SELECT place_id,parent_id FROM place_parent_stage ORDER BY place_id,parent_id;
             INSERT OR IGNORE INTO place_country(place_id,country_id)
                 SELECT place_id,country_id FROM place_country_stage ORDER BY place_id,country_id;
             INSERT OR IGNORE INTO wikidata_candidate(kind,normalized_name,wikidata_id)
                 SELECT kind,normalized_name,wikidata_id FROM wikidata_candidate_stage ORDER BY kind,normalized_name,wikidata_id;
             INSERT OR IGNORE INTO wikidata_place_candidate(normalized_name,wikidata_id)
                 SELECT normalized_name,wikidata_id FROM wikidata_place_candidate_stage ORDER BY normalized_name,wikidata_id;
             INSERT OR IGNORE INTO wikidata_instance(wikidata_id,class_id)
                 SELECT wikidata_id,class_id FROM wikidata_instance_stage ORDER BY wikidata_id,class_id;
             INSERT OR IGNORE INTO wikidata_subclass(child_id,parent_id)
                 SELECT child_id,parent_id FROM wikidata_subclass_stage ORDER BY child_id,parent_id;
             DROP TABLE entity_stage;
             DROP TABLE entity_alias_stage;
             DROP TABLE place_parent_stage;
             DROP TABLE place_country_stage;
             DROP TABLE wikidata_candidate_stage;
             DROP TABLE wikidata_place_candidate_stage;
             DROP TABLE wikidata_instance_stage;
             DROP TABLE wikidata_subclass_stage;",
        )
        .map_err(error)?;
    mark_materialized(&transaction, "wikidata_materialize")?;
    transaction.commit().map_err(error)
}

pub(crate) fn checkpoint_records(connection: &Connection, phase: &str) -> Result<i64, MetadataError> {
    Ok(connection.query_row("SELECT records_read FROM import_checkpoint WHERE phase=?1", [phase], |row| row.get(0)).optional().map_err(error)?.unwrap_or(0))
}

pub(crate) fn wikidata_json_line(line: &str) -> Option<&str> {
    let raw = line.trim().trim_start_matches('[').trim_end_matches(']').trim_end_matches(',').trim();
    (!raw.is_empty()).then_some(raw)
}

pub(crate) fn evidence_targets(connection: &Connection, kind: i64) -> Result<HashSet<String>, MetadataError> {
    let mut statement = connection.prepare("SELECT normalized_name FROM edition_entity_evidence WHERE kind=?1 UNION SELECT normalized_name FROM work_entity_evidence WHERE kind=?1").map_err(error)?;
    let targets = statement.query_map([kind], |row| row.get(0)).map_err(error)?.collect::<Result<HashSet<_>, _>>().map_err(error)?;
    Ok(targets)
}

struct ChildReader {
    child: Child,
    stdout: ChildStdout,
    finished: bool,
}

impl Read for ChildReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.stdout.read(buffer)?;
        if count == 0 && !self.finished {
            let status = self.child.wait()?;
            self.finished = true;
            if !status.success() {
                return Err(io::Error::other(format!("parallel decompressor exited with {status}")));
            }
        }
        Ok(count)
    }
}

impl Drop for ChildReader {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(crate) fn child_reader(mut child: Child) -> Result<Box<dyn BufRead>, MetadataError> {
    let stdout = child.stdout.take().ok_or_else(|| MetadataError("decompressor did not provide stdout".to_owned()))?;
    Ok(Box::new(BufReader::with_capacity(1024 * 1024, ChildReader { child, stdout, finished: false })))
}

pub(crate) fn wikidata_reader(path: &Path, resume_at: usize) -> Result<(Box<dyn BufRead>, bool), MetadataError> {
    if resume_at > 0 {
        if let (Some(seeker), Some(index)) = (std::env::var_os("BOKHEIM_WIKIDATA_SEEKER"), std::env::var_os("BOKHEIM_WIKIDATA_SEEK_INDEX")) {
            if !Path::new(&index).is_file() {
                return wikidata_sequential_reader(path);
            }
            let child = Command::new(seeker).arg("stream").arg(path).arg(index).arg(resume_at.to_string()).args(["--threads", "2"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(error)?;
            return child_reader(child).map(|reader| (reader, true));
        }
    }
    wikidata_sequential_reader(path)
}

pub(crate) fn wikidata_sequential_reader(path: &Path) -> Result<(Box<dyn BufRead>, bool), MetadataError> {
    if path.extension().and_then(|extension| extension.to_str()) == Some("bz2") {
        if let Some(decompressor) = std::env::var_os("BOKHEIM_WIKIDATA_DECOMPRESSOR") {
            let child = Command::new(decompressor).args(["-n", "2", "-dc"]).arg(path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(error)?;
            return child_reader(child).map(|reader| (reader, false));
        }
    }
    let file = File::open(path).map_err(error)?;
    let reader: Box<dyn Read> = match path.extension().and_then(|extension| extension.to_str()) {
        Some("bz2") => Box::new(MultiBzDecoder::new(BufReader::with_capacity(1024 * 1024, file))),
        Some("gz") => Box::new(MultiGzDecoder::new(BufReader::with_capacity(1024 * 1024, file))),
        _ => Box::new(file),
    };
    Ok((Box::new(BufReader::with_capacity(1024 * 1024, reader)), false))
}

struct WikidataEntityRow {
    kind: i64,
    label: String,
    birth_year: Option<i32>,
    death_year: Option<i32>,
    fictional: bool,
    latitude_e7: Option<i32>,
    longitude_e7: Option<i32>,
}

struct WikidataExtract {
    qid: i64,
    entity: Option<WikidataEntityRow>,
    aliases: Vec<String>,
    parents: Vec<i64>,
    countries: Vec<i64>,
    candidates: Vec<(i64, String)>,
    place_candidates: Vec<String>,
    instances: Vec<i64>,
    subclasses: Vec<i64>,
}

fn extract_wikidata_entity(value: WikidataRecord, place_targets: &HashSet<String>, person_targets: &HashSet<String>) -> Option<WikidataExtract> {
    let qid = wikidata_id(&value.id)?;
    let labels = value.labels.values().map(|label| &label.value).collect::<Vec<_>>();
    let aliases = value.aliases.values().flatten().map(|alias| &alias.value).collect::<Vec<_>>();
    let label = preferred_wikidata_label(&value).unwrap_or_else(|| format!("Q{qid}"));
    let instances = claim_entity_ids(&value.claims.instances);
    let subclasses = claim_entity_ids(&value.claims.subclasses);
    let parents = claim_entity_ids(&value.claims.parents);
    let countries = claim_entity_ids(&value.claims.countries);
    let continents = claim_entity_ids(&value.claims.continents);
    let coordinates = claim_coordinate(&value.claims.coordinates);
    let is_human = instances.contains(&5);
    let fictional = instances.contains(&95074);
    // P17/P30 alone also occur on organizations and events. Keep those rows
    // temporarily for ancestor traversal, but do not let them seed a match.
    let explicit_place_type = instances.iter().any(|id| matches!(id, 515 | 6256 | 5107 | 82794 | 486972 | 35657));
    let is_geography_record = coordinates.is_some() || !parents.is_empty() || !countries.is_empty() || !continents.is_empty() || explicit_place_type;
    let is_place_candidate = explicit_place_type || !parents.is_empty() || (coordinates.is_some() && (!countries.is_empty() || !continents.is_empty()));
    let mut matched_place_names = BTreeSet::new();
    let mut matched_person_names = BTreeSet::new();
    for name in labels.iter().chain(&aliases) {
        let normalized = normalize_entity_heading(OL_PLACE_KIND, name);
        if place_targets.contains(&normalized) {
            matched_place_names.insert(normalized.clone());
        }
        if person_targets.contains(&normalized) {
            matched_person_names.insert(normalized);
        }
    }
    let matches_place = !matched_place_names.is_empty();
    let matches_person = !matched_person_names.is_empty();
    let kind = if is_geography_record || (matches_place && !instances.is_empty()) {
        Some(OL_PLACE_KIND)
    } else if (is_human || fictional) && matches_person {
        Some(OL_PERSON_KIND)
    } else {
        None
    };
    let entity = kind.map(|kind| {
        let (latitude_e7, longitude_e7) = coordinates.unzip();
        WikidataEntityRow { kind, label, birth_year: claim_year(&value.claims.births), death_year: claim_year(&value.claims.deaths), fictional, latitude_e7, longitude_e7 }
    });
    let retained_aliases = if entity.is_some() { aliases.into_iter().filter(|alias| !alias.is_empty() && alias.len() <= 4_096).cloned().collect() } else { Vec::new() };
    let (retained_parents, retained_countries) = if kind == Some(OL_PLACE_KIND) { (parents.into_iter().chain(continents).collect(), countries) } else { (Vec::new(), Vec::new()) };
    let mut candidates = Vec::new();
    if is_place_candidate {
        candidates.extend(matched_place_names.iter().cloned().map(|name| (OL_PLACE_KIND, name)));
    }
    if is_human || fictional {
        candidates.extend(matched_person_names.into_iter().map(|name| (OL_PERSON_KIND, name)));
    }
    let extracted = WikidataExtract {
        qid,
        entity,
        aliases: retained_aliases,
        parents: retained_parents,
        countries: retained_countries,
        candidates,
        place_candidates: matched_place_names.into_iter().collect(),
        instances: if matches_place { instances } else { Vec::new() },
        subclasses,
    };
    (extracted.entity.is_some() || !extracted.candidates.is_empty() || !extracted.place_candidates.is_empty() || !extracted.instances.is_empty() || !extracted.subclasses.is_empty()).then_some(extracted)
}

fn preferred_wikidata_label(value: &WikidataRecord) -> Option<String> {
    for language in ["sv", "en"] {
        if let Some(label) = value.labels.get(language) {
            return Some(label.value.clone());
        }
    }
    value.labels.values().next().map(|label| label.value.clone())
}

pub(crate) fn claim_values(claims: &[WikidataClaim]) -> impl Iterator<Item = &WikidataClaimValue> {
    claims.iter().filter_map(|claim| claim.mainsnak.datavalue.as_ref().map(|datavalue| &datavalue.value))
}

pub(crate) fn claim_entity_ids(claims: &[WikidataClaim]) -> Vec<i64> {
    claim_values(claims).filter_map(|value| value.id.as_deref().and_then(wikidata_id).or(value.numeric_id)).collect()
}

fn claim_coordinate(claims: &[WikidataClaim]) -> Option<(i32, i32)> {
    let coordinate = claim_values(claims).next()?;
    let latitude = (coordinate.latitude? * 10_000_000.0).round();
    let longitude = (coordinate.longitude? * 10_000_000.0).round();
    Some((latitude as i32, longitude as i32))
}

fn claim_year(claims: &[WikidataClaim]) -> Option<i32> {
    let time = claim_values(claims).next()?.time.as_deref()?;
    let digits = time.trim_start_matches('+').split('-').next()?;
    digits.parse().ok()
}

pub(crate) fn wikidata_id(value: &str) -> Option<i64> {
    value.strip_prefix('Q')?.parse().ok()
}
