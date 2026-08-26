use super::{
    canonical_isbn13, error, open_library_id, AuthorIdentifier, AuthorMetadata, AuthorSource, BTreeMap, BTreeSet, Classification, ClassificationScheme, ClassificationSource, Connection, EntityReference, EntitySource,
    IsbnClassificationResult, IsbnMetadataResult, LookupStatus, MetadataError, PersonEntity, PlaceEntity, RichIsbnMetadataResult, RichWorkMetadataMatch, WorkClassificationMatch, WorkMetadataMatch, DDC_SCHEME, LCC_SCHEME, OL_PERSON_KIND,
    OL_PLACE_KIND,
};
use crate::import::wikidata::wikidata_id;

const MAX_DIRECT_WORK_CLASSIFICATIONS_WITHOUT_CONSENSUS: usize = 12;

pub(crate) fn lookup_one(connection: &Connection, requested_isbn: String) -> Result<IsbnClassificationResult, MetadataError> {
    let Some(isbn13) = canonical_isbn13(&requested_isbn) else {
        return Ok(IsbnClassificationResult { requested_isbn, canonical_isbn13: None, status: LookupStatus::InvalidIsbn, matches: Vec::new() });
    };
    let mut editions = connection
        .prepare_cached("SELECT edition.edition_id,edition.work_id FROM edition_isbn JOIN edition ON edition.edition_id=edition_isbn.edition_id WHERE edition_isbn.isbn13=?1 ORDER BY edition.work_id,edition.edition_id")
        .map_err(error)?;
    let rows = editions.query_map([isbn13], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))).map_err(error)?;
    let mut by_work = BTreeMap::<(Option<i64>, i64), Vec<i64>>::new();
    for row in rows {
        let (edition_id, work_id) = row.map_err(error)?;
        by_work.entry((work_id, work_id.unwrap_or(edition_id))).or_default().push(edition_id);
    }
    let mut matches = Vec::with_capacity(by_work.len());
    for ((work_id, _group_id), edition_ids) in by_work {
        let mut classifications = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
        for edition_id in &edition_ids {
            collect_classifications(connection, "edition_classification", "edition_id", *edition_id, ClassificationSource::ExactEdition, &mut classifications)?;
        }
        if let Some(work_id) = work_id {
            let mut imported_work_classifications = BTreeMap::new();
            collect_classifications(connection, "work_classification", "work_id", work_id, ClassificationSource::Work, &mut imported_work_classifications)?;
            // Open Library's work record is the authority signal that makes
            // its edition-level union useful. Without it, require independent
            // sibling consensus so one noisy edition cannot classify a work.
            if !imported_work_classifications.is_empty() && imported_work_classifications.len() <= MAX_DIRECT_WORK_CLASSIFICATIONS_WITHOUT_CONSENSUS {
                collect_classifications(connection, "edition_work_classification", "work_id", work_id, ClassificationSource::Work, &mut imported_work_classifications)?;
                for (classification, source) in imported_work_classifications {
                    classifications.entry(classification).or_insert(source);
                }
            }
            for classification in collect_sibling_edition_consensus(connection, work_id)? {
                classifications.entry((classification.scheme, classification.notation)).or_insert(ClassificationSource::Work);
            }
        }
        let classifications = metadata_contract::filter_classification_outliers(classifications.into_iter().map(|((scheme, notation), source)| Classification { scheme, notation, source }), classification_similarity_keys);
        matches.push(WorkClassificationMatch { open_library_work_id: work_id.map(|work_id| format!("OL{work_id}W")), exact_edition_ids: edition_ids.into_iter().map(|edition_id| format!("OL{edition_id}M")).collect(), classifications });
    }
    let status = match matches.len() {
        0 => LookupStatus::NoMatch,
        1 => LookupStatus::Matched,
        _ => LookupStatus::Ambiguous,
    };
    Ok(IsbnClassificationResult { requested_isbn, canonical_isbn13: Some(format!("{isbn13:013}")), status, matches })
}

pub(crate) fn lookup_metadata_one(connection: &Connection, requested_isbn: String) -> Result<IsbnMetadataResult, MetadataError> {
    let classification = lookup_one(connection, requested_isbn)?;
    let matches = classification
        .matches
        .into_iter()
        .map(|matched| WorkMetadataMatch { open_library_work_id: matched.open_library_work_id, exact_edition_ids: matched.exact_edition_ids, classifications: matched.classifications, authors: Vec::new() })
        .collect();
    Ok(IsbnMetadataResult { requested_isbn: classification.requested_isbn, canonical_isbn13: classification.canonical_isbn13, status: classification.status, matches })
}

pub(crate) fn attach_rich_fields(connection: &Connection, metadata: Vec<IsbnMetadataResult>, descriptions: &BTreeMap<i64, String>, bisac: &BTreeMap<i64, Vec<Classification>>) -> Result<Vec<RichIsbnMetadataResult>, MetadataError> {
    let edition_ids = metadata.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.exact_edition_ids).filter_map(|value| open_library_id(value, 'M')).collect::<BTreeSet<_>>();
    let work_ids = metadata.iter().flat_map(|result| &result.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
    let edition_evidence = load_entity_evidence(connection, "edition_entity_evidence", "edition_id", &edition_ids)?;
    let work_evidence = load_entity_evidence(connection, "work_entity_evidence", "work_id", &work_ids)?;
    let popularity = load_work_popularities(connection, &work_ids)?;

    metadata
        .into_iter()
        .map(|result| {
            let matches = result
                .matches
                .into_iter()
                .map(|matched| {
                    let mut places = BTreeMap::<(String, Option<i64>), EntitySource>::new();
                    let mut people = BTreeMap::<(String, Option<i64>), EntitySource>::new();
                    for edition_id in matched.exact_edition_ids.iter().filter_map(|value| open_library_id(value, 'M')) {
                        merge_entity_evidence(edition_evidence.get(&edition_id), EntitySource::ExactEdition, &mut places, &mut people);
                    }
                    let work_id = matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'));
                    if let Some(work_id) = work_id {
                        merge_entity_evidence(work_evidence.get(&work_id), EntitySource::Work, &mut places, &mut people);
                    }
                    let references =
                        |evidence: BTreeMap<(String, Option<i64>), EntitySource>| evidence.into_iter().map(|((name, wikidata_id), source)| EntityReference { wikidata_id: wikidata_id.map(|id| format!("Q{id}")), name, source }).collect();
                    let mut classifications = matched.classifications;
                    classifications.extend(work_id.and_then(|work_id| bisac.get(&work_id)).into_iter().flatten().cloned());
                    RichWorkMetadataMatch {
                        open_library_work_id: matched.open_library_work_id,
                        exact_edition_ids: matched.exact_edition_ids,
                        classifications,
                        authors: matched.authors,
                        places: references(places),
                        people: references(people),
                        popularity: work_id.and_then(|work_id| popularity.get(&work_id).cloned()),
                        description: work_id.and_then(|work_id| descriptions.get(&work_id).cloned()),
                    }
                })
                .collect();
            Ok(RichIsbnMetadataResult { requested_isbn: result.requested_isbn, canonical_isbn13: result.canonical_isbn13, status: result.status, matches })
        })
        .collect()
}

pub(crate) fn load_embedded_work_descriptions(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, String>, MetadataError> {
    let table_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='work_description')", (), |row| row.get::<_, bool>(0)).map_err(error)?;
    if !table_exists {
        return Ok(BTreeMap::new());
    }
    load_work_descriptions_from_table(connection, ids)
}

pub(crate) fn load_work_descriptions_from_table(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, String>, MetadataError> {
    let mut descriptions = BTreeMap::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT work_id,description FROM work_description WHERE work_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
        for row in rows {
            let (work_id, description) = row.map_err(error)?;
            descriptions.insert(work_id, description);
        }
    }
    Ok(descriptions)
}

pub(crate) fn load_work_bisac_classifications(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Vec<Classification>>, MetadataError> {
    let mut classifications = BTreeMap::<i64, Vec<Classification>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT DISTINCT work_id,code FROM work_bisac_subject WHERE work_id IN ({}) ORDER BY work_id,code", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
        for row in rows {
            let (work_id, notation) = row.map_err(error)?;
            classifications.entry(work_id).or_default().push(Classification { scheme: ClassificationScheme::Bisac, notation, source: ClassificationSource::Work });
        }
    }
    Ok(classifications)
}

pub(crate) fn load_work_popularities(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, metadata_contract::WorkPopularity>, MetadataError> {
    let mut popularity = BTreeMap::new();
    let table_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='work_popularity')", (), |row| row.get::<_, bool>(0)).map_err(error)?;
    if !table_exists {
        return Ok(popularity);
    }
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT work_id,want_to_read_count,currently_reading_count,already_read_count,rating_count,rating_sum FROM work_popularity WHERE work_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    metadata_contract::WorkPopularity {
                        want_to_read_count: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                        currently_reading_count: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                        already_read_count: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                        rating_count: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                        rating_sum: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                    },
                ))
            })
            .map_err(error)?;
        for row in rows {
            let (work_id, value) = row.map_err(error)?;
            popularity.insert(work_id, value);
        }
    }
    Ok(popularity)
}

pub(crate) fn collect_response_entities(connection: &Connection, results: &[RichIsbnMetadataResult]) -> Result<(Vec<PlaceEntity>, Vec<PersonEntity>), MetadataError> {
    let mut pending_places = results.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.places).filter_map(|reference| reference.wikidata_id.as_deref().and_then(wikidata_id)).collect::<BTreeSet<_>>();
    let person_ids = results.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.people).filter_map(|reference| reference.wikidata_id.as_deref().and_then(wikidata_id)).collect::<BTreeSet<_>>();
    let mut seen_places = BTreeSet::new();
    let mut places = Vec::new();
    loop {
        let batch = pending_places.difference(&seen_places).copied().collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        seen_places.extend(batch.iter().copied());
        let loaded = load_place_entities(connection, &batch)?;
        for place in loaded {
            pending_places.extend(place.parent_ids.iter().chain(&place.country_ids).filter_map(|id| wikidata_id(id)));
            places.push(place);
        }
    }
    places.sort_by(|left, right| left.wikidata_id.cmp(&right.wikidata_id));
    let mut people = load_person_entities(connection, &person_ids.into_iter().collect::<Vec<_>>())?;
    people.sort_by(|left, right| left.wikidata_id.cmp(&right.wikidata_id));
    Ok((places, people))
}

fn load_place_entities(connection: &Connection, ids: &[i64]) -> Result<Vec<PlaceEntity>, MetadataError> {
    let mut entities = BTreeMap::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT wikidata_id,label,latitude_e7,longitude_e7 FROM entity WHERE kind={OL_PLACE_KIND} AND wikidata_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<i32>>(2)?, row.get::<_, Option<i32>>(3)?))).map_err(error)?;
        for row in rows {
            let (id, label, latitude, longitude) = row.map_err(error)?;
            entities.insert(id, (label, latitude, longitude));
        }
    }
    let aliases = load_entity_aliases(connection, ids)?;
    let parents = load_entity_edges(connection, "place_parent", "place_id", "parent_id", ids)?;
    let countries = load_entity_edges(connection, "place_country", "place_id", "country_id", ids)?;
    Ok(entities
        .into_iter()
        .map(|(id, (label, latitude_e7, longitude_e7))| PlaceEntity {
            wikidata_id: format!("Q{id}"),
            label,
            aliases: aliases.get(&id).cloned().unwrap_or_default(),
            parent_ids: parents.get(&id).into_iter().flatten().map(|value| format!("Q{value}")).collect(),
            country_ids: countries.get(&id).into_iter().flatten().map(|value| format!("Q{value}")).collect(),
            latitude_e7,
            longitude_e7,
        })
        .collect())
}

fn load_person_entities(connection: &Connection, ids: &[i64]) -> Result<Vec<PersonEntity>, MetadataError> {
    let aliases = load_entity_aliases(connection, ids)?;
    let mut people = Vec::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT wikidata_id,label,birth_year,death_year,fictional FROM entity WHERE kind={OL_PERSON_KIND} AND wikidata_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows =
            statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<i32>>(2)?, row.get::<_, Option<i32>>(3)?, row.get::<_, bool>(4)?))).map_err(error)?;
        for row in rows {
            let (id, label, birth_year, death_year, fictional) = row.map_err(error)?;
            people.push(PersonEntity { wikidata_id: format!("Q{id}"), label, aliases: aliases.get(&id).cloned().unwrap_or_default(), birth_year, death_year, fictional });
        }
    }
    Ok(people)
}

pub(crate) fn load_entity_aliases(connection: &Connection, ids: &[i64]) -> Result<BTreeMap<i64, Vec<String>>, MetadataError> {
    let mut aliases = BTreeMap::<i64, Vec<String>>::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT wikidata_id,alias FROM entity_alias WHERE wikidata_id IN ({}) ORDER BY wikidata_id,alias", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
        for row in rows {
            let (id, alias) = row.map_err(error)?;
            aliases.entry(id).or_default().push(alias);
        }
    }
    Ok(aliases)
}

pub(crate) fn load_entity_edges(connection: &Connection, table: &str, source_column: &str, target_column: &str, ids: &[i64]) -> Result<BTreeMap<i64, Vec<i64>>, MetadataError> {
    let mut edges = BTreeMap::<i64, Vec<i64>>::new();
    for chunk in ids.chunks(500) {
        let sql = format!("SELECT {source_column},{target_column} FROM {table} WHERE {source_column} IN ({}) ORDER BY {source_column},{target_column}", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).map_err(error)?;
        for row in rows {
            let (source, target) = row.map_err(error)?;
            edges.entry(source).or_default().push(target);
        }
    }
    Ok(edges)
}

pub(crate) fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count).collect::<Vec<_>>().join(",")
}

pub(crate) fn load_entity_evidence(connection: &Connection, table: &str, id_column: &str, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Vec<(i64, String, Option<i64>)>>, MetadataError> {
    let mut evidence = BTreeMap::<i64, Vec<(i64, String, Option<i64>)>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT {id_column},kind,name,wikidata_id FROM {table} WHERE {id_column} IN ({}) ORDER BY {id_column},kind,name", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<i64>>(3)?))).map_err(error)?;
        for row in rows {
            let (owner_id, kind, name, wikidata_id) = row.map_err(error)?;
            evidence.entry(owner_id).or_default().push((kind, name, wikidata_id));
        }
    }
    Ok(evidence)
}

pub(crate) fn merge_entity_evidence(evidence: Option<&Vec<(i64, String, Option<i64>)>>, source: EntitySource, places: &mut BTreeMap<(String, Option<i64>), EntitySource>, people: &mut BTreeMap<(String, Option<i64>), EntitySource>) {
    for (kind, name, wikidata_id) in evidence.into_iter().flatten() {
        let key = (name.clone(), *wikidata_id);
        match *kind {
            OL_PLACE_KIND => {
                places.entry(key).and_modify(|existing| *existing = (*existing).min(source)).or_insert(source);
            }
            OL_PERSON_KIND => {
                people.entry(key).and_modify(|existing| *existing = (*existing).min(source)).or_insert(source);
            }
            _ => {}
        }
    }
}

pub(crate) fn attach_authors(connection: &Connection, results: &mut [IsbnMetadataResult]) -> Result<(), MetadataError> {
    let edition_ids = results.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.exact_edition_ids).filter_map(|value| open_library_id(value, 'M')).collect::<BTreeSet<_>>();
    let work_ids = results.iter().flat_map(|result| &result.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
    let edition_authors = load_author_relations(connection, "edition_author", "edition_id", &edition_ids)?;
    let work_authors = load_author_relations(connection, "work_author", "work_id", &work_ids)?;
    let author_ids = edition_authors.values().chain(work_authors.values()).flatten().map(|(author_id, _)| *author_id).collect::<BTreeSet<_>>();
    let names = load_author_names(connection, &author_ids)?;
    let identifiers = load_author_identifiers(connection, &author_ids)?;

    for matched in results.iter_mut().flat_map(|result| &mut result.matches) {
        let mut authors = BTreeMap::<i64, (AuthorSource, u32)>::new();
        for edition_id in matched.exact_edition_ids.iter().filter_map(|value| open_library_id(value, 'M')) {
            merge_author_relations(&mut authors, edition_authors.get(&edition_id), AuthorSource::ExactEdition);
        }
        if let Some(work_id) = matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W')) {
            merge_author_relations(&mut authors, work_authors.get(&work_id), AuthorSource::Work);
        }
        matched.authors = authors
            .into_iter()
            .map(|(author_id, (source, position))| AuthorMetadata {
                open_library_author_id: format!("OL{author_id}A"),
                name: names.get(&author_id).cloned().flatten(),
                biography: None,
                identifiers: identifiers.get(&author_id).cloned().unwrap_or_default(),
                source,
                position,
            })
            .collect();
        matched.authors.sort_by_key(|author| (author.source, author.position, author.open_library_author_id.clone()));
    }
    Ok(())
}

pub(crate) fn load_author_relations(connection: &Connection, table: &str, id_column: &str, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Vec<(i64, u32)>>, MetadataError> {
    let mut relations = BTreeMap::<i64, Vec<(i64, u32)>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT {id_column},author_id,position FROM {table} WHERE {id_column} IN ({}) ORDER BY {id_column},position", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))).map_err(error)?;
        for row in rows {
            let (owner_id, author_id, position) = row.map_err(error)?;
            relations.entry(owner_id).or_default().push((author_id, u32::try_from(position).map_err(error)?));
        }
    }
    Ok(relations)
}

pub(crate) fn merge_author_relations(authors: &mut BTreeMap<i64, (AuthorSource, u32)>, relations: Option<&Vec<(i64, u32)>>, source: AuthorSource) {
    for &(author_id, position) in relations.into_iter().flatten() {
        authors
            .entry(author_id)
            .and_modify(|existing| {
                if (source, position) < *existing {
                    *existing = (source, position);
                }
            })
            .or_insert((source, position));
    }
}

fn load_author_names(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Option<String>>, MetadataError> {
    let mut names = BTreeMap::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT author_id,name FROM author WHERE author_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))).map_err(error)?;
        for row in rows {
            let (author_id, name) = row.map_err(error)?;
            names.insert(author_id, name);
        }
    }
    Ok(names)
}

pub(crate) fn load_author_biographies(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, String>, MetadataError> {
    let mut biographies = BTreeMap::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT author_id,biography FROM author_profile WHERE author_id IN ({})", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
        for row in rows {
            let (author_id, biography) = row.map_err(error)?;
            biographies.insert(author_id, biography);
        }
    }
    Ok(biographies)
}

fn load_author_identifiers(connection: &Connection, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Vec<AuthorIdentifier>>, MetadataError> {
    let mut identifiers = BTreeMap::<i64, Vec<AuthorIdentifier>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT author_id,authority,external_id FROM author_identifier WHERE author_id IN ({}) ORDER BY author_id,authority,external_id", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, AuthorIdentifier { authority: row.get(1)?, value: row.get(2)? }))).map_err(error)?;
        for row in rows {
            let (author_id, identifier) = row.map_err(error)?;
            identifiers.entry(author_id).or_default().push(identifier);
        }
    }
    Ok(identifiers)
}

fn collect_classifications(connection: &Connection, table: &str, id_column: &str, id: i64, source: ClassificationSource, classifications: &mut BTreeMap<(ClassificationScheme, String), ClassificationSource>) -> Result<(), MetadataError> {
    let sql = format!("SELECT scheme,notation FROM {table} WHERE {id_column}=?1 ORDER BY scheme,notation");
    let mut statement = connection.prepare_cached(&sql).map_err(error)?;
    let rows = statement.query_map([id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
    for row in rows {
        let (scheme, notation) = row.map_err(error)?;
        let scheme = match scheme {
            DDC_SCHEME => ClassificationScheme::DeweyDecimal,
            LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
            _ => continue,
        };
        classifications.entry((scheme, notation)).and_modify(|existing| *existing = (*existing).min(source)).or_insert(source);
    }
    Ok(())
}

fn collect_sibling_edition_consensus(connection: &Connection, work_id: i64) -> Result<Vec<Classification>, MetadataError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT edition_classification.edition_id,edition_classification.scheme,edition_classification.notation
             FROM edition_classification
             JOIN edition ON edition.edition_id=edition_classification.edition_id
             WHERE edition.work_id=?1
             ORDER BY edition_classification.edition_id,edition_classification.scheme,edition_classification.notation",
        )
        .map_err(error)?;
    let rows = statement.query_map([work_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
    let mut support = BTreeMap::<(ClassificationScheme, String), BTreeSet<i64>>::new();
    for row in rows {
        let (edition_id, scheme, notation) = row.map_err(error)?;
        let scheme = match scheme {
            DDC_SCHEME => ClassificationScheme::DeweyDecimal,
            LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
            _ => continue,
        };
        let Some(notation) = metadata_contract::classification_consensus_notation(scheme, &notation) else { continue };
        support.entry((scheme, notation)).or_default().insert(edition_id);
    }
    Ok(support.into_iter().filter_map(|((scheme, notation), editions)| (editions.len() >= 2).then_some(Classification { scheme, notation, source: ClassificationSource::Work })).collect())
}

pub(crate) fn classification_similarity_keys(scheme: ClassificationScheme, notation: &str) -> Vec<String> {
    let system_id = match scheme {
        ClassificationScheme::LibraryOfCongress => subject_projection::LCC_SYSTEM_ID,
        ClassificationScheme::DeweyDecimal => subject_projection::DDC_SYSTEM_ID,
        ClassificationScheme::Bisac => subject_projection::BISAC_SYSTEM_ID,
    };
    subject_projection::unified_subject_similarity_keys(system_id, notation)
}
