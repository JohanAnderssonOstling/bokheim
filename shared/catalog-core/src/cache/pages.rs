use super::{
    active_generation_optional, next_sequence_in, params, CachedCatalogPage, CatalogCache, CatalogCacheError, CatalogLinkTarget, CatalogPage, CatalogPublication, CatalogSourceId, Connection, HashSet, OptionalExtension, Transaction,
};

impl CatalogCache {
    pub fn page(&self, source_id: &CatalogSourceId, locator: &str) -> Result<Option<CachedCatalogPage>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(None);
        };
        connection
            .query_row("SELECT payload, etag, last_modified, provider_paginated FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
                let payload: Vec<u8> = row.get(0)?;
                let page = decode_page(&payload).map_err(sql_decode_error)?;
                Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)?, provider_paginated: row.get(3)? })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn has_pages(&self, source_id: &CatalogSourceId) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(false) };
        connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2)", params![source_id.as_str(), storage_i64(generation, "generation")?], |row| row.get(0)).map_err(Into::into)
    }

    /// Checks for unfinished graph discovery without materializing every
    /// missing locator. The crawler walks cached pages into its own temporary
    /// disk frontier when this returns true.
    pub fn has_missing_page_frontier(&self, source_id: &CatalogSourceId, root_locator: &str) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(true) };
        let generation = storage_i64(generation, "generation")?;
        let root_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3)", params![source_id.as_str(), generation, root_locator], |row| row.get::<_, bool>(0))?;
        if !root_exists {
            return Ok(true);
        }
        connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM catalog_page_link link
                     LEFT JOIN catalog_page page
                       ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
                     WHERE link.source_id=?1 AND link.generation=?2 AND page.locator IS NULL
                 )",
                params![source_id.as_str(), generation],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Returns only links whose target page is absent from the active
    /// generation. Page payloads remain in SQLite.
    pub fn missing_page_frontier(&self, source_id: &CatalogSourceId, root_locator: &str) -> Result<Vec<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(vec![root_locator.to_owned()]) };
        let generation = storage_i64(generation, "generation")?;
        let mut frontier = Vec::new();
        let root_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3)", params![source_id.as_str(), generation, root_locator], |row| row.get::<_, bool>(0))?;
        if !root_exists {
            frontier.push(root_locator.to_owned());
        }
        let mut statement = connection.prepare(
            "SELECT DISTINCT link.to_locator
             FROM catalog_page_link link
             LEFT JOIN catalog_page page
               ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
             WHERE link.source_id=?1 AND link.generation=?2 AND page.locator IS NULL
             ORDER BY link.to_locator",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation], |row| row.get::<_, String>(0))?;
        frontier.extend(rows.collect::<Result<Vec<_>, _>>()?);
        Ok(frontier)
    }

    pub fn pages(&self, source_id: &CatalogSourceId) -> Result<Vec<CachedCatalogPage>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT payload, etag, last_modified, provider_paginated FROM catalog_page
             WHERE source_id=?1 AND generation=?2 ORDER BY locator",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), storage_i64(generation, "generation")?], |row| {
            let payload: Vec<u8> = row.get(0)?;
            let page = decode_page(&payload).map_err(sql_decode_error)?;
            Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)?, provider_paginated: row.get(3)? })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub fn put_page_row(
    transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, page: &CatalogPage, payload: &[u8], etag: Option<&str>, last_modified: Option<&str>, provider_paginated: bool,
) -> Result<(), CatalogCacheError> {
    transaction.execute(
        "INSERT INTO catalog_page(source_id, generation, locator, payload, etag, last_modified, provider_paginated) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(source_id, generation, locator) DO UPDATE SET payload=excluded.payload, etag=excluded.etag, last_modified=excluded.last_modified,provider_paginated=excluded.provider_paginated",
        params![source_id.as_str(), storage_i64(generation, "generation")?, page.locator, payload, etag, last_modified, provider_paginated],
    )?;
    replace_page_links(transaction, source_id.as_str(), storage_i64(generation, "generation")?, &page.locator, page)?;
    Ok(())
}

pub(crate) fn replace_page_links(transaction: &Transaction<'_>, source_id: &str, generation: i64, locator: &str, page: &CatalogPage) -> Result<(), CatalogCacheError> {
    transaction.execute("DELETE FROM catalog_page_link WHERE source_id=?1 AND generation=?2 AND from_locator=?3", params![source_id, generation, locator])?;
    let mut seen = HashSet::new();
    let navigation = page.navigation.iter().chain(page.groups.iter().flat_map(|group| group.navigation.iter())).filter_map(|link| match &link.target {
        CatalogLinkTarget::Page { locator } => Some(locator.as_str()),
        CatalogLinkTarget::External { .. } => None,
    });
    for target in navigation
        .chain(page.groups.iter().filter_map(|group| group.page_locator.as_deref()))
        .chain([page.pagination.first.as_deref(), page.pagination.previous.as_deref(), page.pagination.next.as_deref(), page.pagination.last.as_deref()].into_iter().flatten())
    {
        if seen.insert(target) {
            transaction.execute("INSERT OR IGNORE INTO catalog_page_link(source_id,generation,from_locator,to_locator) VALUES(?1,?2,?3,?4)", params![source_id, generation, locator, target])?;
        }
    }
    Ok(())
}

pub(crate) fn page_in(connection: &Connection, source_id: &CatalogSourceId, generation: u64, locator: &str) -> Result<Option<CachedCatalogPage>, CatalogCacheError> {
    connection
        .query_row("SELECT payload, etag, last_modified, provider_paginated FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
            let payload: Vec<u8> = row.get(0)?;
            let page = decode_page(&payload).map_err(sql_decode_error)?;
            Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)?, provider_paginated: row.get(3)? })
        })
        .optional()
        .map_err(Into::into)
}

pub fn publication_key(publication: &CatalogPublication) -> (&str, &str) {
    (&publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or(""))
}

pub fn allocate_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<i64, CatalogCacheError> {
    let next = next_sequence_in(transaction, source_id)?.checked_add(1).ok_or_else(|| CatalogCacheError::new("publication sequence overflow"))?;
    transaction.execute("UPDATE catalog_source SET next_sequence=?2 WHERE source_id=?1", params![source_id.as_str(), storage_i64(next, "publication sequence")?])?;
    storage_i64(next, "publication sequence")
}

pub fn sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &catalog_model::CatalogPublicationIdentity) -> Result<Option<u64>, CatalogCacheError> {
    transaction
        .query_row(
            "SELECT sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            |row| stored_u64(row.get(0)?, "publication sequence"),
        )
        .optional()
        .map_err(Into::into)
}

pub(crate) fn decode_page(payload: &[u8]) -> Result<CatalogPage, wire::WireError> {
    decode_cached(payload)
}

pub fn decode_cached<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, wire::WireError> {
    wire::decode(payload, wire::MAX_DECODED_RESPONSE_BYTES)
}

pub fn sql_decode_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error))
}

pub fn storage_i64(value: u64, name: &str) -> Result<i64, CatalogCacheError> {
    i64::try_from(value).map_err(|_| CatalogCacheError::new(format!("{name} exceeds SQLite integer range")))
}

pub fn stored_u64(value: i64, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} is negative")))))
}

pub fn optional_stored_u64(value: Option<i64>, name: &str) -> rusqlite::Result<Option<u64>> {
    value.map(|value| stored_u64(value, name)).transpose()
}
