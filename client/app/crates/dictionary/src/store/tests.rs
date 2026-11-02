use std::fs::{self, File};
use std::io::{self, SeekFrom};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use std::path::PathBuf;
#[derive(Clone)]
struct TestStorage {
    root: PathBuf,
    bytes_read: Arc<AtomicUsize>,
}
impl TestStorage {
    fn new(root: PathBuf) -> Self {
        Self { root, bytes_read: Arc::new(AtomicUsize::new(0)) }
    }
}
struct MeteredFile {
    file: File,
    bytes_read: Arc<AtomicUsize>,
}
impl Read for MeteredFile {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(output)?;
        self.bytes_read.fetch_add(count, Ordering::Relaxed);
        Ok(count)
    }
}
impl Seek for MeteredFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
impl Storage for TestStorage {
    type Source = MeteredFile;
    fn read_value(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match fs::read(self.root.join(key)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    fn write_value(&self, key: &str, bytes: &[u8]) -> Result<()> {
        fs::write(self.root.join(key), bytes).map_err(|error| error.to_string())
    }
    async fn open(&self, id: &str, name: &str) -> Result<Self::Source> {
        Ok(MeteredFile { file: native::open(&self.root, id, name).await?, bytes_read: self.bytes_read.clone() })
    }
    async fn write_all(&self, id: &str, files: Vec<(String, Vec<u8>)>) -> Result<()> {
        native::write_all(&self.root, id, files).await
    }
    async fn remove(&self, id: &str) -> Result<()> {
        native::remove(&self.root, id).await
    }
}

fn dictionary(name: &str, definition: &str) -> Vec<DictionaryFile> {
    dictionary_word(name, "book", definition, "")
}

fn dictionary_word(name: &str, word: &str, definition: &str, language: &str) -> Vec<DictionaryFile> {
    let mut index = format!("{word}\0").into_bytes();
    index.extend_from_slice(&0u32.to_be_bytes());
    index.extend_from_slice(&(definition.len() as u32).to_be_bytes());
    let ifo = format!("StarDict's dict ifo file\nversion=2.4.2\nbookname={name}\nwordcount=1\nidxfilesize={}\nsametypesequence=m\nidxlang={language}\n", index.len());
    [("ifo", ifo.into_bytes()), ("idx", index), ("dict", definition.as_bytes().to_vec())].into_iter().map(|(suffix, bytes)| DictionaryFile { name: format!("custom.{suffix}"), bytes }).collect()
}

#[tokio::test]
async fn dictionary_import_priority_disabled_state_and_removal_survive_reopening() {
    let root = tempfile::tempdir().unwrap();
    let context = TestStorage::new(root.path().to_owned());
    let backend = Arc::new(DictionaryStore::new(context.clone()));
    let catalog = backend.import(dictionary("First", "A printed volume")).await.unwrap();
    let first = catalog[0].id.clone();
    let catalog = backend.import(dictionary("Second", "A reservation")).await.unwrap();
    let second = catalog[1].id.clone();
    assert_eq!(backend.lookup("BOOK".into(), Vec::new()).await.unwrap().dictionaries[0].dictionary, "First");
    backend.configure(second.clone(), true, -1).await.unwrap();
    backend.configure(first.clone(), false, 0).await.unwrap();
    assert!(backend.import(vec![]).await.is_err());
    assert_eq!(backend.list().await.unwrap().len(), 2);
    drop(backend);
    let backend = Arc::new(DictionaryStore::new(context));
    let found = backend.lookup("book".into(), Vec::new()).await.unwrap();
    assert_eq!(found.enabled_count, 1);
    assert_eq!(found.dictionaries[0].dictionary, "Second");
    assert_eq!(found.dictionaries[0].definitions[0].text, "A reservation");
    backend.remove(second.clone()).await.unwrap();
    assert!(!root.path().join("dictionaries-v1").join(second).exists());
    assert_eq!(backend.lookup("book".into(), Vec::new()).await.unwrap().enabled_count, 0);
    let installed = backend.list().await.unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, first);
}

#[tokio::test]
async fn failed_file_install_leaves_the_catalog_unchanged_and_allows_retry() {
    let root = tempfile::tempdir().unwrap();
    let context = TestStorage::new(root.path().to_owned());
    let backend = Arc::new(DictionaryStore::new(context));
    let directory = root.path().join("dictionaries-v1");
    fs::write(&directory, b"Blocks directory creation").unwrap();
    assert!(backend.import(dictionary("Example", "A printed volume")).await.is_err());
    assert!(backend.list().await.unwrap().is_empty());
    fs::remove_file(directory).unwrap();
    assert_eq!(backend.import(dictionary("Example", "A printed volume")).await.unwrap().len(), 1);
    assert_eq!(backend.lookup("book".into(), Vec::new()).await.unwrap().dictionaries[0].definitions[0].text, "A printed volume");
}

#[tokio::test]
async fn lookup_leaves_installed_files_and_catalog_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(DictionaryStore::new(TestStorage::new(root.path().to_owned())));
    let catalog = backend.import(dictionary("Example", "A printed volume")).await.unwrap();
    let persisted = fs::read(root.path().join(CATALOG)).unwrap();
    assert_eq!(backend.lookup("book".into(), Vec::new()).await.unwrap().dictionaries.len(), 1);
    assert_eq!(fs::read(root.path().join(CATALOG)).unwrap(), persisted);

    let index = root.path().join("dictionaries-v1").join(&catalog[0].id).join(IDX_FILE);
    fs::write(&index, b"old format").unwrap();
    let found = backend.lookup("book".into(), Vec::new()).await.unwrap();
    assert!(found.dictionaries.is_empty());
    assert_eq!(found.errors.len(), 1);
    assert_eq!(fs::read(index).unwrap(), b"old format");
    assert_eq!(fs::read(root.path().join(CATALOG)).unwrap(), persisted);
}

#[tokio::test]
async fn source_languages_prioritize_matches_and_unknowns_with_foreign_word_fallback() {
    let root = tempfile::tempdir().unwrap();
    let context = TestStorage::new(root.path().to_owned());
    let backend = Arc::new(DictionaryStore::new(context));
    let first = backend.import(dictionary_word("English homonym", "Haus", "English meaning", "en")).await.unwrap()[0].id.clone();
    backend.import(dictionary_word("German–English", "Haus", "House", "deu")).await.unwrap();
    backend.import(dictionary_word("Unknown", "Haus", "Custom meaning", "")).await.unwrap();
    backend.import(dictionary_word("English", "book", "A printed volume", "en")).await.unwrap();
    let found = backend.lookup("Haus".into(), vec!["de-DE".into()]).await.unwrap();
    assert_eq!(found.dictionaries.iter().map(|item| item.dictionary.as_str()).collect::<Vec<_>>(), ["German–English", "Unknown"]);
    let foreign = backend.lookup("book".into(), vec!["de-DE".into()]).await.unwrap();
    assert_eq!(foreign.dictionaries[0].dictionary, "English");
    let untagged = backend.lookup("Haus".into(), Vec::new()).await.unwrap();
    assert_eq!(untagged.dictionaries.len(), 3);
    // A primary match must not open a known other-language dictionary.
    fs::remove_file(root.path().join("dictionaries-v1").join(first).join(DICT_FILE)).unwrap();
    assert!(backend.lookup("Haus".into(), vec!["de".into()]).await.unwrap().errors.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn disabling_and_removing_a_curated_pack_preserve_user_choices_after_reopening() {
    let root = tempfile::tempdir().unwrap();
    let context = TestStorage::new(root.path().to_owned());
    let backend = Arc::new(DictionaryStore::new(context.clone()));
    let spec = dictionary_catalog::for_language("de").unwrap();
    let mut index = b"Haus\0".to_vec();
    index.extend_from_slice(&0u32.to_be_bytes());
    index.extend_from_slice(&4u32.to_be_bytes());
    let ifo = format!("StarDict's dict ifo file\nversion=2.4.2\nbookname=German\nwordcount=1\nidxfilesize={}\nsametypesequence=m\n", index.len());
    let files = [("ifo", ifo.into_bytes()), ("idx", index), ("dict", b"Home".to_vec())].into_iter().map(|(suffix, bytes)| DictionaryFile { name: format!("fixture.{suffix}"), bytes }).collect();
    let installed = backend.install(files, Some(spec)).await.unwrap();
    let id = installed[0].id.clone();
    let installed = backend.configure(id.clone(), false, 0).await.unwrap();
    assert!(!installed[0].enabled);
    drop(backend);
    let backend = Arc::new(DictionaryStore::new(context.clone()));
    assert!(!backend.list().await.unwrap()[0].enabled);
    backend.remove(id).await.unwrap();
    drop(backend);
    let backend = Arc::new(DictionaryStore::new(context));
    assert!(backend.list().await.unwrap().is_empty());
    assert!(backend.downloads(&["de-DE".into()]).await.unwrap().active.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn recommendation_languages_are_request_local_and_never_install_dictionaries() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(DictionaryStore::new(TestStorage::new(root.path().to_owned())));
    for language in ["de-DE", "sv-SE"] {
        let snapshot = backend.downloads(&[language.into()]).await.unwrap();
        assert_eq!(snapshot.recommended, vec![dictionary_catalog::for_language(language).unwrap().id.clone()]);
        assert!(snapshot.active.is_none());
        assert!(snapshot.error.is_none());
    }
    assert!(backend.downloads(&[]).await.unwrap().recommended.is_empty());
    assert_eq!(backend.lookup("Häuser".into(), Vec::new()).await.unwrap().enabled_count, 0);
    assert!(backend.list().await.unwrap().is_empty());
    assert!(fs::read_dir(root.path()).unwrap().next().is_none(), "recommendations must not write preferences or dictionary files");
}

#[tokio::test]
async fn obsolete_installs_can_be_listed_and_removed_without_migration() {
    let root = tempfile::tempdir().unwrap();
    let context = TestStorage::new(root.path().to_owned());
    let backend = Arc::new(DictionaryStore::new(context));
    let mut installed = backend.import(dictionary("Old dictionary", "A printed volume")).await.unwrap();
    let id = installed[0].id.clone();
    installed[0].files = vec!["dictionary.search".into(), "dictionary.data".into(), "dictionary.json".into()];
    backend.save_catalog(&installed).unwrap();
    assert_eq!(backend.list().await.unwrap().len(), 1);
    let found = backend.lookup("book".into(), Vec::new()).await.unwrap();
    assert!(found.dictionaries.is_empty());
    assert!(found.errors[0].contains("imported or downloaded again"));
    assert!(backend.remove(id).await.unwrap().is_empty());
}

#[tokio::test]
async fn offset_tables_are_reused_and_evicted_on_disable_and_removal() {
    let root = tempfile::tempdir().unwrap();
    let context = TestStorage::new(root.path().to_owned());
    let bytes_read = context.bytes_read.clone();
    let backend = Arc::new(DictionaryStore::new(context));
    let first = backend.import(dictionary("First", "A printed volume")).await.unwrap()[0].id.clone();
    let second = backend.import(dictionary("Second", "A reservation")).await.unwrap()[1].id.clone();
    assert_eq!(backend.lookup("book".into(), Vec::new()).await.unwrap().dictionaries.len(), 2);
    let cold_reads = bytes_read.swap(0, Ordering::Relaxed);
    assert_eq!(backend.storage.lock().await.len(), 2);
    backend.lookup("BOOK".into(), Vec::new()).await.unwrap();
    assert!(bytes_read.load(Ordering::Relaxed) < cold_reads, "warm lookups must reuse offsets instead of reading complete indexes and metadata again");
    backend.configure(first.clone(), false, 0).await.unwrap();
    assert!(!backend.storage.lock().await.contains_key(&first));
    backend.remove(second).await.unwrap();
    assert!(backend.storage.lock().await.is_empty());
}
