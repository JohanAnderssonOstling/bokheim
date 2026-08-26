use super::*;

const MAX_CATALOG_LOCATOR_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub(super) struct ServerRegistryStore {
    path: std::path::PathBuf,
    registry: CatalogSourceRegistry,
}

impl ServerRegistryStore {
    pub(super) fn load(path: std::path::PathBuf) -> Self {
        let registry = std::fs::read(&path)
            .ok()
            .filter(|bytes| bytes.len() as u64 <= MAX_CATALOG_SERVER_RESPONSE_BYTES)
            .and_then(|bytes| serde_json::from_slice::<CatalogSourceRegistry>(&bytes).ok())
            .filter(|registry| validate_registry(registry).is_ok())
            .unwrap_or(CatalogSourceRegistry { revision: 0, sources: Vec::new() });
        Self { path, registry }
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

    pub(super) fn replace(&mut self, registry: CatalogSourceRegistry) -> Result<(), CatalogProviderError> {
        validate_registry(&registry)?;
        if registry.revision < self.registry.revision {
            return Err(CatalogProviderError::new(format!("refusing stale catalogue registry revision {} because revision {} is cached", registry.revision, self.registry.revision)));
        }
        if registry.revision == self.registry.revision && self.registry.revision != 0 && registry != self.registry {
            return Err(CatalogProviderError::new(format!("catalogue registry revision {} changed without a revision increment", registry.revision)));
        }
        persist_registry(&self.path, &registry)?;
        self.registry = registry;
        Ok(())
    }
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

fn persist_registry(path: &Path, registry: &CatalogSourceRegistry) -> Result<(), CatalogProviderError> {
    let parent = path.parent().ok_or_else(|| CatalogProviderError::new("catalogue registry cache path has no parent directory"))?;
    std::fs::create_dir_all(parent).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    serde_json::to_writer(temporary.as_file_mut(), registry).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    temporary.as_file_mut().sync_all().map_err(|error| CatalogProviderError::new(error.to_string()))?;
    temporary.persist(path).map_err(|error| CatalogProviderError::new(error.error.to_string()))?;
    Ok(())
}
