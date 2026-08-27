use super::*;
use rusqlite::OptionalExtension;

const MAX_CATALOG_LOCATOR_BYTES: usize = 16 * 1024;

pub(super) struct ServerRegistryStore {
    registry: CatalogSourceRegistry,
}

impl ServerRegistryStore {
    pub(super) fn load(store: &client_store::ClientCatalogStore) -> Result<Self, CatalogProviderError> {
        let connection = store.connection.lock().map_err(|_| CatalogProviderError::new("catalogue client store lock was poisoned"))?;
        let payload = connection.query_row("SELECT payload FROM catalog_client_registry WHERE singleton=1", [], |row| row.get::<_, Vec<u8>>(0)).optional().map_err(error)?;
        let registry = payload
            .filter(|bytes| bytes.len() as u64 <= MAX_CATALOG_SERVER_RESPONSE_BYTES)
            .and_then(|bytes| serde_json::from_slice::<CatalogSourceRegistry>(&bytes).ok())
            .filter(|registry| validate_registry(registry).is_ok())
            .unwrap_or(CatalogSourceRegistry { revision: 0, sources: Vec::new() });
        Ok(Self { registry })
    }

    pub(super) fn sources(&self) -> Vec<CatalogSource> {
        self.registry.sources.iter().map(|registration| registration.source.clone()).collect()
    }

    pub(super) fn registrations(&self) -> Vec<CatalogSourceRegistration> {
        self.registry.sources.clone()
    }

    pub(super) fn registration(&self, source_id: &CatalogSourceId) -> Option<CatalogSourceRegistration> {
        self.registry.sources.iter().find(|registration| &registration.source.id == source_id).cloned()
    }

    pub(super) fn replace(&mut self, store: &client_store::ClientCatalogStore, registry: CatalogSourceRegistry) -> Result<(), CatalogProviderError> {
        validate_registry(&registry)?;
        if registry.revision < self.registry.revision {
            return Err(CatalogProviderError::new(format!("refusing stale catalogue registry revision {} because revision {} is cached", registry.revision, self.registry.revision)));
        }
        if registry.revision == self.registry.revision && self.registry.revision != 0 && registry != self.registry {
            return Err(CatalogProviderError::new(format!("catalogue registry revision {} changed without a revision increment", registry.revision)));
        }
        let bytes = serde_json::to_vec(&registry).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        store
            .connection
            .lock()
            .map_err(|_| CatalogProviderError::new("catalogue client store lock was poisoned"))?
            .execute(
                "INSERT INTO catalog_client_registry(singleton,payload) VALUES(1,?1)
                 ON CONFLICT(singleton) DO UPDATE SET payload=excluded.payload",
                [&bytes],
            )
            .map_err(error)?;
        self.registry = registry;
        Ok(())
    }
}

fn error(error: impl std::fmt::Display) -> CatalogProviderError {
    CatalogProviderError::new(error.to_string())
}

pub(super) fn validate_locator(locator: &str) -> Result<(), CatalogProviderError> {
    if locator.is_empty() || locator.len() > MAX_CATALOG_LOCATOR_BYTES || locator.chars().any(char::is_control) {
        return Err(CatalogProviderError::new("catalogue page locator must be non-empty bounded text without control characters"));
    }
    Ok(())
}

fn validate_registry(registry: &CatalogSourceRegistry) -> Result<(), CatalogProviderError> {
    if registry.revision == 0 {
        return Err(CatalogProviderError::new("catalogue registry revision must be greater than zero"));
    }
    if registry.sources.is_empty() {
        return Err(CatalogProviderError::new("catalogue registry must contain at least one source"));
    }
    let mut source_ids = HashSet::new();
    for registration in &registry.sources {
        if !source_ids.insert(registration.source.id.clone()) {
            return Err(CatalogProviderError::new("catalogue registry contains duplicate source ids"));
        }
        validate_locator(&registration.root_locator)?;
    }
    Ok(())
}
