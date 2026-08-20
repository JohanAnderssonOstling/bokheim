use super::{
    attach_authors, attach_rich_fields, collect_response_entities, error, load_author_biographies, load_embedded_work_descriptions, load_work_bisac_classifications, load_work_descriptions_from_table, load_work_popularities,
    lookup_metadata_one, lookup_one, open_description_pool, open_library_id, open_read_pool, query_connection, resolve_edition_batch, Arc, AtomicU64, BTreeMap, BTreeSet, Classification, ClassificationRequest, ClassificationResponse,
    Connection, EditionIdentityRequest, EditionIdentityResponse, EndpointMetrics, Instant, IsbnMetadataResult, MetadataEnrichmentRequest, MetadataEnrichmentResponse, MetadataError, MetadataService, PathBuf, QueryConnection,
    RichIsbnMetadataResult, RichMetadataEnrichmentResponse, RichWorkMetadataMatch, Semaphore, ServiceState, SnapshotDescription, AUTHOR_PROFILE_SCHEMA_VERSION, DESCRIPTION_SCHEMA_VERSION, LEGACY_QUERY_SCHEMA_VERSION,
    MAX_EDITION_QUERIES_PER_REQUEST, MAX_ISBNS_PER_REQUEST, QUERY_CONCURRENCY, SCHEMA_VERSION, WIKIDATA_SCHEMA_VERSION,
};
use crate::http::validate_isbn_count;

impl MetadataService {
    pub fn open(database: impl Into<PathBuf>) -> Result<Self, MetadataError> {
        Self::open_with_descriptions(database, None::<PathBuf>)
    }

    pub fn open_with_descriptions(database: impl Into<PathBuf>, description_database: Option<impl Into<PathBuf>>) -> Result<Self, MetadataError> {
        Self::open_with_supplements(database, description_database, None::<PathBuf>)
    }

    pub fn open_with_supplements(database: impl Into<PathBuf>, description_database: Option<impl Into<PathBuf>>, author_profile_database: Option<impl Into<PathBuf>>) -> Result<Self, MetadataError> {
        let database = database.into();
        let pool = open_read_pool(&database)?;
        let (schema_version, dump_date, imported_at_ms, embedded_descriptions_available) = {
            let connection = pool.get().map_err(error)?;
            let snapshot = connection.query_row("SELECT schema_version,dump_date,imported_at_ms FROM snapshot WHERE singleton=1", (), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))).map_err(error)?;
            let descriptions_available = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='work_description')", (), |row| row.get::<_, bool>(0)).map_err(error)?;
            (snapshot.0, snapshot.1, snapshot.2, descriptions_available)
        };
        if !matches!(schema_version, LEGACY_QUERY_SCHEMA_VERSION | SCHEMA_VERSION | WIKIDATA_SCHEMA_VERSION) {
            return Err(MetadataError(format!("unsupported metadata database schema {schema_version}; expected {LEGACY_QUERY_SCHEMA_VERSION}, {SCHEMA_VERSION}, or {WIKIDATA_SCHEMA_VERSION}")));
        }
        let description_pool = description_database
            .map(Into::into)
            .map(|description_database| {
                let description_pool = open_description_pool(&description_database)?;
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
        let descriptions_available = description_pool.is_some() || embedded_descriptions_available;
        let author_profile_pool = author_profile_database
            .map(Into::into)
            .map(|author_profile_database| {
                let author_profile_pool = open_description_pool(&author_profile_database)?;
                let connection = author_profile_pool.get().map_err(error)?;
                let (author_schema, author_dump_date): (i64, String) = connection.query_row("SELECT schema_version,dump_date FROM author_profile_snapshot WHERE singleton=1", (), |row| Ok((row.get(0)?, row.get(1)?))).map_err(error)?;
                if author_schema != AUTHOR_PROFILE_SCHEMA_VERSION {
                    return Err(MetadataError(format!("unsupported author-profile sidecar schema {author_schema}; expected {AUTHOR_PROFILE_SCHEMA_VERSION}")));
                }
                if author_dump_date != dump_date {
                    return Err(MetadataError(format!("author-profile sidecar dump date {author_dump_date} does not match metadata snapshot {dump_date}")));
                }
                drop(connection);
                Ok(author_profile_pool)
            })
            .transpose()?;
        let imported_at_ms = u64::try_from(imported_at_ms).map_err(|_| MetadataError("metadata snapshot has an invalid import timestamp".to_owned()))?;
        Ok(Self {
            state: Arc::new(ServiceState {
                database,
                pool,
                description_pool,
                author_profile_pool,
                query_slots: Arc::new(Semaphore::new(QUERY_CONCURRENCY as usize)),
                snapshot: SnapshotDescription { dump_date, imported_at_ms },
                schema_version,
                descriptions_available,
                started_at: Instant::now(),
                requests: AtomicU64::new(0),
                errors: AtomicU64::new(0),
                total_duration_ms: AtomicU64::new(0),
                processed_items: AtomicU64::new(0),
                item_processing_duration_ms: AtomicU64::new(0),
                edition_identities: EndpointMetrics::default(),
                classifications: EndpointMetrics::default(),
                enrichment: EndpointMetrics::default(),
            }),
        })
    }

    pub fn lookup(&self, request: ClassificationRequest) -> Result<ClassificationResponse, MetadataError> {
        if request.isbns.is_empty() {
            return Err(MetadataError("at least one ISBN is required".to_owned()));
        }
        if request.isbns.len() > MAX_ISBNS_PER_REQUEST {
            return Err(MetadataError(format!("at most {MAX_ISBNS_PER_REQUEST} ISBNs may be requested")));
        }
        let connection = self.connection()?;
        let mut results = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            results.push(lookup_one(&connection, requested_isbn)?);
        }
        Ok(ClassificationResponse { snapshot: self.state.snapshot.clone(), results })
    }

    pub fn enrich(&self, request: MetadataEnrichmentRequest) -> Result<MetadataEnrichmentResponse, MetadataError> {
        validate_isbn_count(request.isbns.len())?;
        let connection = self.connection()?;
        let mut results = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            results.push(lookup_metadata_one(&connection, requested_isbn)?);
        }
        attach_authors(&connection, &mut results)?;
        Ok(MetadataEnrichmentResponse { snapshot: self.state.snapshot.clone(), results })
    }

    pub fn enrich_rich(&self, request: MetadataEnrichmentRequest) -> Result<RichMetadataEnrichmentResponse, MetadataError> {
        validate_isbn_count(request.isbns.len())?;
        let connection = self.connection()?;
        let mut metadata = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            metadata.push(lookup_metadata_one(&connection, requested_isbn)?);
        }
        attach_authors(&connection, &mut metadata)?;
        self.attach_author_biographies(&mut metadata)?;
        let work_ids = metadata.iter().flat_map(|result| &result.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
        let (descriptions, bisac) = self.load_work_supplements(&connection, &work_ids)?;
        let results = attach_rich_fields(&connection, metadata, &descriptions, &bisac)?;
        let (places, people) = collect_response_entities(&connection, &results)?;
        Ok(RichMetadataEnrichmentResponse { snapshot: self.state.snapshot.clone(), results, places, people })
    }

    pub fn classify_rich(&self, request: MetadataEnrichmentRequest) -> Result<RichMetadataEnrichmentResponse, MetadataError> {
        validate_isbn_count(request.isbns.len())?;
        let connection = self.connection()?;
        let mut classifications = Vec::with_capacity(request.isbns.len());
        for requested_isbn in request.isbns {
            classifications.push(lookup_one(&connection, requested_isbn)?);
        }
        let work_ids = classifications.iter().flat_map(|classification| &classification.matches).filter_map(|matched| matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'))).collect::<BTreeSet<_>>();
        let popularity = load_work_popularities(&connection, &work_ids)?;
        let (descriptions, bisac) = self.load_work_supplements(&connection, &work_ids)?;
        let mut results = Vec::with_capacity(classifications.len());
        for classification in classifications {
            let matches = classification
                .matches
                .into_iter()
                .map(|matched| {
                    let work_id = matched.open_library_work_id.as_deref().and_then(|value| open_library_id(value, 'W'));
                    let mut classifications = matched.classifications;
                    classifications.extend(work_id.and_then(|work_id| bisac.get(&work_id)).into_iter().flatten().cloned());
                    RichWorkMetadataMatch {
                        open_library_work_id: matched.open_library_work_id,
                        exact_edition_ids: matched.exact_edition_ids,
                        classifications,
                        authors: Vec::new(),
                        places: Vec::new(),
                        people: Vec::new(),
                        popularity: work_id.and_then(|work_id| popularity.get(&work_id).cloned()),
                        description: work_id.and_then(|work_id| descriptions.get(&work_id).cloned()),
                    }
                })
                .collect();
            results.push(RichIsbnMetadataResult { requested_isbn: classification.requested_isbn, canonical_isbn13: classification.canonical_isbn13, status: classification.status, matches });
        }
        Ok(RichMetadataEnrichmentResponse { snapshot: self.state.snapshot.clone(), results, places: Vec::new(), people: Vec::new() })
    }

    pub fn resolve_editions(&self, request: EditionIdentityRequest) -> Result<EditionIdentityResponse, MetadataError> {
        if request.queries.is_empty() || request.queries.len() > MAX_EDITION_QUERIES_PER_REQUEST {
            return Err(MetadataError(format!("between 1 and {MAX_EDITION_QUERIES_PER_REQUEST} edition queries are required")));
        }
        let connection = self.connection()?;
        let results = resolve_edition_batch(&connection, request.queries)?;
        Ok(EditionIdentityResponse { snapshot: self.state.snapshot.clone(), results })
    }

    pub(crate) fn connection(&self) -> Result<QueryConnection, MetadataError> {
        query_connection(&self.state.pool)
    }

    fn load_work_supplements(&self, metadata_connection: &Connection, ids: &BTreeSet<i64>) -> Result<(BTreeMap<i64, String>, BTreeMap<i64, Vec<Classification>>), MetadataError> {
        if let Some(pool) = &self.state.description_pool {
            let connection = query_connection(pool)?;
            let descriptions = load_work_descriptions_from_table(&connection, ids)?;
            let classifications = load_work_bisac_classifications(&connection, ids)?;
            return Ok((descriptions, classifications));
        }
        Ok((load_embedded_work_descriptions(metadata_connection, ids)?, BTreeMap::new()))
    }

    fn attach_author_biographies(&self, results: &mut [IsbnMetadataResult]) -> Result<(), MetadataError> {
        let Some(pool) = &self.state.author_profile_pool else { return Ok(()) };
        let author_ids = results.iter().flat_map(|result| &result.matches).flat_map(|matched| &matched.authors).filter_map(|author| open_library_id(&author.open_library_author_id, 'A')).collect::<BTreeSet<_>>();
        let connection = query_connection(pool)?;
        let biographies = load_author_biographies(&connection, &author_ids)?;
        for author in results.iter_mut().flat_map(|result| &mut result.matches).flat_map(|matched| &mut matched.authors) {
            author.biography = open_library_id(&author.open_library_author_id, 'A').and_then(|author_id| biographies.get(&author_id).cloned());
        }
        Ok(())
    }
}
