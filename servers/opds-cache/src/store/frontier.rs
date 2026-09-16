use super::*;

impl CatalogStore {
    pub fn page(&self, source_id: &CatalogSourceId, locator: &str) -> Result<Option<CachedCatalogPage>, CatalogStoreError> {
        let connection = self.connection()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(None);
        };
        connection
            .query_row("SELECT payload,etag,last_modified,provider_paginated FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
                let payload: Vec<u8> = row.get(0)?;
                let page = decode_cached(&payload).map_err(sql_decode_error)?;
                Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)?, provider_paginated: row.get(3)? })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn has_pages(&self, source_id: &CatalogSourceId) -> Result<bool, CatalogStoreError> {
        let connection = self.connection()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(false) };
        connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2)", params![source_id.as_str(), storage_i64(generation, "generation")?], |row| row.get(0)).map_err(Into::into)
    }

    /// Checks for unfinished graph discovery without materializing every
    /// missing locator. The crawler walks cached pages into its own temporary
    /// disk frontier when this returns true.
    pub fn has_missing_page_frontier(&self, source_id: &CatalogSourceId, root_locator: &str) -> Result<bool, CatalogStoreError> {
        let connection = self.connection()?;
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
}

pub(crate) fn put_page_row(
    transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, page: &CatalogPage, payload: &[u8], etag: Option<&str>, last_modified: Option<&str>, provider_paginated: bool,
) -> Result<(), CatalogStoreError> {
    let generation = storage_i64(generation, "generation")?;
    transaction.execute(
        "INSERT INTO catalog_page(source_id,generation,locator,payload,etag,last_modified,provider_paginated) VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(source_id,generation,locator) DO UPDATE SET payload=excluded.payload,etag=excluded.etag,last_modified=excluded.last_modified,provider_paginated=excluded.provider_paginated",
        params![source_id.as_str(), generation, page.locator, payload, etag, last_modified, provider_paginated],
    )?;
    replace_page_links(transaction, source_id.as_str(), generation, &page.locator, page)
}

fn replace_page_links(transaction: &Transaction<'_>, source_id: &str, generation: i64, locator: &str, page: &CatalogPage) -> Result<(), CatalogStoreError> {
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
