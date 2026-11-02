//! Dictionary lifecycle and lookup, independent of application sessions.
use crate::{Body, DICT_FILE, DICTZIP_FILE, Definition, Dictionary, DictionaryFile, DictionaryInfo, DictionaryLookup, DictionaryMatches, IDX_FILE, IFO_FILE, MAX_INDEX_BYTES, Result, SYN_FILE, bounded_read, can_lookup, prepare_import};
use client_platform_runtime::executor::{BackendSend, run_blocking};
use std::collections::{HashMap, hash_map::Entry};
use std::future::Future;
use std::io::{Read, Seek};
use std::sync::{Arc, Mutex as SyncMutex};
use tokio::sync::{Mutex, watch};

pub(crate) mod downloads;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
const CATALOG: &str = "dictionaries-v1.json";
// Old names remain accepted so obsolete installations can still be removed.
const COMPONENTS: &[&str] = &[IFO_FILE, IDX_FILE, SYN_FILE, DICT_FILE, DICTZIP_FILE, "dictionary.license.txt", "dictionary.attribution.txt", "dictionary.search", "dictionary.data", "dictionary.json", "dictionary.idx.gz"];

/// App-owned preferences and platform file access. Dictionary policy stays here.
pub trait Storage: Clone + 'static {
    type Source: Read + Seek;
    fn read_value(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn write_value(&self, key: &str, bytes: &[u8]) -> Result<()>;
    fn open(&self, id: &str, name: &str) -> impl Future<Output = Result<Self::Source>> + BackendSend;
    fn write_all(&self, id: &str, files: Vec<(String, Vec<u8>)>) -> impl Future<Output = Result<()>> + BackendSend;
    fn remove(&self, id: &str) -> impl Future<Output = Result<()>> + BackendSend;
}
pub struct DictionaryStore<S> {
    wake: watch::Sender<()>,
    download_lock: Mutex<()>,
    activity: SyncMutex<downloads::Activity>,
    storage_context: S,
    storage: Mutex<HashMap<String, Dictionary>>,
}
impl<S: Storage> DictionaryStore<S> {
    pub fn new(storage_context: S) -> Self {
        Self { storage_context, storage: Mutex::new(HashMap::new()), wake: watch::channel(()).0, download_lock: Mutex::new(()), activity: SyncMutex::new(Default::default()) }
    }
    pub fn changes(&self) -> watch::Receiver<()> {
        self.wake.subscribe()
    }
    fn save_catalog(&self, catalog: &[DictionaryInfo]) -> Result<()> {
        self.storage_context.write_value(CATALOG, &serde_json::to_vec_pretty(catalog).map_err(|error| error.to_string())?)
    }
    fn catalog(&self) -> Result<Vec<DictionaryInfo>> {
        let items: Vec<DictionaryInfo> = self.storage_context.read_value(CATALOG)?.map(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string())).transpose()?.unwrap_or_default();
        // Validate persisted locators before allowing filesystem access.
        for item in &items {
            uuid::Uuid::parse_str(&item.id).map_err(|_| String::from("Invalid dictionary identifier"))?;
            if !item.files.iter().all(|name| COMPONENTS.contains(&name.as_str())) {
                return Err(String::from("Invalid dictionary component name"));
            }
        }
        Ok(items)
    }
    pub async fn list(&self) -> Result<Vec<DictionaryInfo>> {
        let _guard = self.storage.lock().await;
        self.catalog()
    }

    pub async fn import(&self, input: Vec<DictionaryFile>) -> Result<Vec<DictionaryInfo>> {
        self.install(input, None).await
    }

    async fn install(&self, input: Vec<DictionaryFile>, curated: Option<&dictionary_catalog::CuratedDictionary>) -> Result<Vec<DictionaryInfo>> {
        let (mut metadata, components) = run_blocking(move || prepare_import(input)).await.map_err(|error| error.to_string())??;
        let _guard = self.storage.lock().await;
        let mut catalog = self.catalog()?;
        if let Some(spec) = curated {
            if catalog.iter().any(|item| item.curated_id.as_deref() == Some(spec.id.as_str())) {
                return Ok(catalog);
            }
            metadata.language = Some(spec.language.clone());
            metadata.description.push_str(&format!("\n\n{} · {}\n{}\n{}", spec.source, spec.license, spec.source_url, spec.upstream_url));
            if let Some(version) = spec.license.strip_prefix("CC-BY-SA-") {
                metadata.description.push_str(&format!("\nhttps://creativecommons.org/licenses/by-sa/{version}/\nWiktionary contributors — https://{}.wiktionary.org/", spec.language));
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let size_bytes = components.iter().map(|file| file.bytes.len() as u64).sum();
        let names = components.iter().map(|file| file.name.clone()).collect();
        let result = async {
            let components = components.into_iter().map(|file| (file.name, file.bytes)).collect();
            self.storage_context.write_all(&id, components).await?;
            catalog.push(DictionaryInfo { id: id.clone(), metadata, enabled: true, size_bytes, files: names, curated_id: curated.map(|spec| spec.id.clone()) });
            self.save_catalog(&catalog)?;
            self.wake.send_replace(());
            Ok(catalog)
        }
        .await;
        if result.is_err() {
            let _ = self.storage_context.remove(&id).await;
        }
        result
    }

    /// Priority is the catalog order. A move changes one slot at a time.
    pub async fn configure(&self, id: String, enabled: bool, direction: i8) -> Result<Vec<DictionaryInfo>> {
        let mut indexes = self.storage.lock().await;
        let mut catalog = self.catalog()?;
        let index = catalog.iter().position(|item| item.id == id).ok_or_else(|| String::from("Dictionary was removed"))?;
        catalog[index].enabled = enabled;
        let next = index.saturating_add_signed(direction.signum() as isize).min(catalog.len() - 1);
        catalog.swap(index, next);
        self.save_catalog(&catalog)?;
        if !enabled {
            indexes.remove(&id);
        }
        self.wake.send_replace(());
        Ok(catalog)
    }

    pub async fn remove(&self, id: String) -> Result<Vec<DictionaryInfo>> {
        let mut indexes = self.storage.lock().await;
        let mut catalog = self.catalog()?;
        if !catalog.iter().any(|item| item.id == id) {
            return Err(String::from("Dictionary was already removed"));
        }
        catalog.retain(|item| item.id != id);
        self.save_catalog(&catalog)?;
        indexes.remove(&id);
        self.wake.send_replace(());
        // Publish removal before deleting files. A failed preference write
        // must leave the installed dictionary usable.
        if let Err(error) = self.storage_context.remove(&id).await {
            log::warn!("Could not clean up removed dictionary {id}: {error}");
        }
        Ok(catalog)
    }

    async fn lookup_inner(&self, query: String, languages: Vec<String>) -> Result<DictionaryLookup> {
        let query = query.trim();
        if !can_lookup(query) {
            return Err(String::from("Select a word or short phrase to look up"));
        }
        let languages: Vec<_> = languages.iter().filter_map(|tag| dictionary_catalog::language(tag)).collect();
        let mut indexes = self.storage.lock().await;
        let catalog = self.catalog()?;
        let mut result = DictionaryLookup { dictionaries: Vec::new(), enabled_count: catalog.iter().filter(|item| item.enabled).count(), errors: Vec::new() };
        // Unknown source languages participate in the primary search. Known
        // other languages are fallback for foreign words in a tagged book.
        for fallback in [false, true] {
            if fallback && (!result.dictionaries.is_empty() || languages.is_empty()) {
                break;
            }
            for item in catalog.iter().filter(|item| item.enabled) {
                let other_language = !languages.is_empty() && item.metadata.language.as_deref().and_then(dictionary_catalog::language).is_some_and(|language| !languages.contains(&language));
                if other_language != fallback {
                    continue;
                }
                let lookup = lookup_installed(self, item, query, &mut indexes).await;
                match lookup {
                    Ok(definitions) if !definitions.is_empty() => result.dictionaries.push(DictionaryMatches { dictionary: item.metadata.name.clone(), definitions }),
                    Ok(_) => {}
                    Err(error) => result.errors.push(format!("{}: {error}", item.metadata.name)),
                }
            }
        }
        Ok(result)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<S: Storage + Send + Sync> DictionaryStore<S> {
    pub async fn lookup(self: &Arc<Self>, query: String, languages: Vec<String>) -> Result<DictionaryLookup> {
        let store = self.clone();
        let runtime = tokio::runtime::Handle::current();
        run_blocking(move || runtime.block_on(store.lookup_inner(query, languages))).await.map_err(|error| error.to_string())?
    }
}
#[cfg(target_arch = "wasm32")]
impl<S: Storage> DictionaryStore<S> {
    pub async fn lookup(self: &Arc<Self>, query: String, languages: Vec<String>) -> Result<DictionaryLookup> {
        self.lookup_inner(query, languages).await
    }
}
async fn lookup_installed<S: Storage>(store: &DictionaryStore<S>, item: &DictionaryInfo, query: &str, indexes: &mut HashMap<String, Dictionary>) -> Result<Vec<Definition>> {
    if !item.files.iter().any(|name| name == IDX_FILE) {
        return Err("Dictionary needs to be imported or downloaded again".into());
    }
    let mut index = store.storage_context.open(&item.id, IDX_FILE).await?;
    let mut synonyms = if item.files.iter().any(|name| name == SYN_FILE) { Some(store.storage_context.open(&item.id, SYN_FILE).await?) } else { None };
    let compressed = item.files.iter().any(|name| name == DICTZIP_FILE);
    let mut body = Body::open(store.storage_context.open(&item.id, if compressed { DICTZIP_FILE } else { DICT_FILE }).await?, compressed)?;
    let dictionary = match indexes.entry(item.id.clone()) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => {
            let ifo = bounded_read(store.storage_context.open(&item.id, IFO_FILE).await?, 1024 * 1024)?;
            let index_bytes = bounded_read(&mut index, MAX_INDEX_BYTES)?;
            let synonym_bytes = synonyms.as_mut().map(|reader| bounded_read(reader, MAX_INDEX_BYTES)).transpose()?;
            entry.insert(Dictionary::open(&ifo, &index_bytes, synonym_bytes.as_deref(), body.len())?)
        }
    };
    dictionary.lookup(query, &mut index, synonyms.as_mut(), &mut body)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
