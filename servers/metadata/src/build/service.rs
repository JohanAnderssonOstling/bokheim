use crate::*;

struct WorkSupplements {
    descriptions: BTreeMap<i64, String>,
    bisac: BTreeMap<i64, Vec<Classification>>,
}

/// The classification lookup as a rich result whose authors, description and
/// subjects the enrichment steps fill in place.
fn classification_to_metadata(classification: crate::IsbnClassificationResult) -> RichIsbnMetadataResult {
    let matches = classification
        .matches
        .into_iter()
        .map(|matched| RichWorkMetadataMatch { open_library_work_id: matched.open_library_work_id, exact_edition_ids: matched.exact_edition_ids, classifications: matched.classifications, authors: Vec::new(), description: None, subjects: Vec::new() })
        .collect();
    RichIsbnMetadataResult { requested_isbn: classification.requested_isbn, canonical_isbn13: classification.canonical_isbn13, status: classification.status, matches }
}

impl Resolver {
    pub(crate) fn open(database: &Path, description_database: Option<&Path>) -> Result<Self, MetadataError> {
        let database = database.to_path_buf();
        let pool = crate::build::openlibrary::open_builder_pool(&database)?;
        let (schema_version, dump_date, imported_at_ms) = {
            let connection = pool.get().map_err(error)?;
            let snapshot = connection.query_row("SELECT schema_version,dump_date,imported_at_ms FROM snapshot WHERE singleton=1", (), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))).map_err(error)?;
            (snapshot.0, snapshot.1, snapshot.2)
        };
        if schema_version != SCHEMA_VERSION {
            return Err(MetadataError(format!("unsupported metadata database schema {schema_version}; expected {SCHEMA_VERSION}")));
        }
        let description_pool = description_database
            .map(|description_database| {
                let description_pool = open_description_pool(description_database)?;
                let connection = description_pool.get().map_err(error)?;
                let (description_schema, description_dump_date): (i64, String) =
                    connection.query_row("SELECT schema_version,dump_date FROM description_snapshot WHERE singleton=1", (), |row| Ok((row.get(0)?, row.get(1)?))).map_err(error)?;
                if description_schema != DESCRIPTION_SCHEMA_VERSION {
                    return Err(MetadataError(format!("unsupported description sidecar schema {description_schema}; expected {DESCRIPTION_SCHEMA_VERSION}")));
                }
                if description_dump_date != dump_date {
                    return Err(MetadataError(format!("description sidecar dump date {description_dump_date} does not match metadata snapshot {dump_date}")));
                }
                drop(connection);
                Ok(description_pool)
            })
            .transpose()?;
        {
            let c = query_connection(&pool)?;
            crate::build::authority_ingestion::available(&c)?;
        }
        let imported_at_ms = u64::try_from(imported_at_ms).map_err(|_| MetadataError("metadata snapshot has an invalid import timestamp".to_owned()))?;
        Ok(Self { state: Arc::new(ResolverState { database, pool, description_pool, snapshot: SnapshotDescription { dump_date, imported_at_ms } }) })
    }

    pub(crate) fn lookup(&self, isbns: Vec<String>) -> Result<Vec<IsbnClassificationResult>, MetadataError> {
        if isbns.is_empty() {
            return Err(MetadataError("at least one ISBN is required".to_owned()));
        }
        if isbns.len() > MAX_ISBNS_PER_REQUEST {
            return Err(MetadataError(format!("at most {MAX_ISBNS_PER_REQUEST} ISBNs may be requested")));
        }
        let connection = self.connection()?;
        let mut results = Vec::with_capacity(isbns.len());
        for requested_isbn in isbns {
            results.push(lookup_one(&connection, requested_isbn)?);
        }
        if results.iter().any(|r| r.matches.len() == 1 && !r.matches[0].classifications.iter().any(|c| c.scheme == metadata_contract::ClassificationScheme::LibraryOfCongress)) {
            let identity = query_connection(&self.state.pool)?;
            for result in &mut results {
                crate::build::duplicate_work::recover(&identity, result)?;
            }
        }
        Ok(results)
    }

    pub(crate) fn enrich_rich(&self, isbns: Vec<String>) -> Result<Vec<RichIsbnMetadataResult>, MetadataError> {
        let mut metadata = self.lookup_metadata(isbns)?;
        let connection = query_connection(&self.state.pool)?;
        attach_authors(&connection, &mut metadata)?;
        let work_ids = metadata.iter().flat_map(|result| &result.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
        let supplements = self.load_work_supplements(&connection, &work_ids)?;
        attach_rich_fields(&connection, &mut metadata, &supplements.descriptions, &supplements.bisac)?;
        crate::build::loc_dump::enrich(&connection, &mut metadata)?;
        Ok(metadata)
    }


    pub(crate) fn connection(&self) -> Result<QueryConnection, MetadataError> {
        query_connection(&self.state.pool)
    }

    fn lookup_metadata(&self, isbns: Vec<String>) -> Result<Vec<RichIsbnMetadataResult>, MetadataError> {
        Ok(self.lookup(isbns)?.into_iter().map(classification_to_metadata).collect())
    }

    fn load_work_supplements(&self, metadata_connection: &Connection, ids: &BTreeSet<i64>) -> Result<WorkSupplements, MetadataError> {
        if let Some(pool) = &self.state.description_pool {
            let connection = query_connection(pool)?;
            let descriptions = load_work_descriptions_from_table(&connection, ids)?;
            let classifications = load_work_bisac_classifications(&connection, ids)?;
            return Ok(WorkSupplements { descriptions, bisac: classifications });
        }
        Ok(WorkSupplements { descriptions: load_embedded_work_descriptions(metadata_connection, ids)?, bisac: BTreeMap::new() })
    }
}
