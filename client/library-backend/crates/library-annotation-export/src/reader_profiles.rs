//! Device-local reader data. Discovery never creates libraries or copies books.
use crate::failure::{failure, failure_details, input};
use library_model::{AnnotationExportFailureKind as Kind, AnnotationExportFormat};
use md5::{Digest, Md5};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use reader_interop::{
    encoded_identifier,
    profiles::{foliate_directories, readest_books_directory, readest_directories},
};
const RETAINED: &str = "_bokheim_retained_reader_notes";

#[derive(Clone, Debug)]
pub(crate) enum ProfileTarget {
    Foliate { path: PathBuf, identifier: String },
    Readest { path: PathBuf, hash: String, meta_hash: Option<String> },
}

impl ProfileTarget {
    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Foliate { path, .. } | Self::Readest { path, .. } => path,
        }
    }

    /// Adapt a native config to the portable reconciliation contract without
    /// treating bookmarks, notebook entries or ink as highlight annotations.
    pub(crate) fn decode(&self, bytes: &[u8], expected: &mut Value) -> io::Result<Vec<u8>> {
        let mut document: Value = serde_json::from_slice(bytes).map_err(input)?;
        if !document.is_object() {
            return Err(failure(Kind::InvalidData, "reader book data must be a JSON object"));
        }
        match self {
            Self::Foliate { identifier, .. } => {
                if let Some(actual) = document.pointer("/metadata/identifier") {
                    if actual.as_str() != Some(identifier) {
                        return Err(failure(Kind::Identity, "Foliate book data has a different publication identifier"));
                    }
                } else {
                    if document.get("metadata").is_none() {
                        document["metadata"] = json!({});
                    }
                    if !document["metadata"].is_object() {
                        return Err(failure(Kind::InvalidData, "invalid Foliate book metadata"));
                    }
                    document["metadata"]["identifier"] = identifier.clone().into();
                }
                if document.get("annotations").is_none() {
                    document["annotations"] = json!([]);
                }
            }
            Self::Readest { hash, .. } => {
                if document.get("bookHash").is_some_and(|value| !value.is_null() && value.as_str() != Some(hash)) {
                    return Err(failure(Kind::Identity, "Readest config belongs to another book"));
                }
                expected["book"]["hash"] = hash.clone().into();
                let notes = document.as_object_mut().unwrap().remove("booknotes").unwrap_or_else(|| json!([]));
                let Value::Array(notes) = notes else {
                    return Err(failure(Kind::InvalidData, "invalid Readest booknotes"));
                };
                if notes.iter().any(|note| !note.is_object()) {
                    return Err(failure(Kind::InvalidData, "invalid Readest note record"));
                }
                let (annotations, retained): (Vec<_>, Vec<_>) = notes.into_iter().partition(|note| note["type"] == "annotation" && note["deletedAt"].is_null());
                document["annotations"] = annotations.into();
                document[RETAINED] = retained.into();
                document["$format"] = "readest-annotations".into();
                document["version"] = 1.into();
                document["book"] = expected["book"].clone();
            }
        }
        // Reader identity is the publication identifier or file hash: the
        // same native book can be shared by several Bokheim libraries.
        document.as_object_mut().unwrap().remove("_bokheim_book_id");
        serde_json::to_vec(&document).map_err(input)
    }

    pub(crate) fn encode(&self, bytes: Vec<u8>, previous: Option<(&Value, &[String])>, exported_at: u64) -> io::Result<Vec<u8>> {
        let mut document: Value = serde_json::from_slice(&bytes).map_err(input)?;
        document.as_object_mut().ok_or_else(|| failure(Kind::InvalidData, "invalid reader config"))?.remove("_bokheim_book_id");
        let Self::Readest { hash, meta_hash, .. } = self else {
            if let Some((previous, _)) = previous {
                if let Some(metadata) = previous["metadata"].as_object() {
                    if let Some(generated) = document["metadata"].as_object_mut() {
                        generated.extend(metadata.clone());
                    }
                }
            }
            return serde_json::to_vec_pretty(&document).map_err(input);
        };
        let mut source_ids = previous
            .into_iter()
            .flat_map(|(document, _)| document["_bokheim_annotation_ids"].as_object().into_iter().flatten().filter_map(|(source, model)| model.as_str().map(|model| (model.to_owned(), source.clone()))))
            .collect::<HashMap<_, _>>();
        source_ids.extend(previous.into_iter().flat_map(|(document, identities)| {
            document["annotations"].as_array().into_iter().flatten().zip(identities).filter(|(_, identity)| !identity.is_empty()).filter_map(|(note, identity)| note["id"].as_str().map(|source| (identity.clone(), source.to_owned())))
        }));
        source_ids.extend(previous.into_iter().flat_map(|(document, _)| document[RETAINED].as_array().into_iter().flatten().filter_map(|note| Some((note["_bokheim_id"].as_str()?.to_owned(), note["id"].as_str()?.to_owned())))));
        let object = document.as_object_mut().ok_or_else(|| failure(Kind::InvalidData, "invalid Readest config"))?;
        let Some(Value::Array(mut annotations)) = object.remove("annotations") else {
            return Err(failure(Kind::InvalidData, "missing Readest annotations"));
        };
        let exported_ids = match object.remove("_bokheim_annotation_ids") {
            Some(Value::Object(ids)) => ids,
            _ => Default::default(),
        };
        let model_ids = annotations.iter().filter_map(|note| note["id"].as_str().filter(|id| exported_ids.contains_key(*id)).map(str::to_owned)).collect::<HashSet<_>>();
        let owned_native_ids = model_ids.iter().filter_map(|id| source_ids.get(id).map(String::as_str)).collect::<HashSet<_>>();
        let external_ids = annotations
            .iter()
            .filter_map(|note| note["id"].as_str().filter(|id| !model_ids.contains(*id)).map(str::to_owned))
            .chain(object.get(RETAINED).and_then(Value::as_array).into_iter().flatten().filter_map(|note| note["id"].as_str().filter(|id| !owned_native_ids.contains(*id)).map(str::to_owned)))
            .collect::<HashSet<_>>();
        let mut ids = serde_json::Map::new();
        for note in &mut annotations {
            let Some(model_id) = note["id"].as_str().filter(|id| model_ids.contains(*id)).map(str::to_owned) else { continue };
            let source_id = source_ids.get(&model_id).cloned().unwrap_or_else(|| model_id.clone());
            if external_ids.contains(&source_id) {
                return Err(failure(Kind::Identity, "an external Readest note uses the export ID; destination left unchanged"));
            }
            note["id"] = source_id.clone().into();
            note["_bokheim_id"] = model_id.clone().into();
            note["bookHash"] = hash.clone().into();
            if let Some(meta_hash) = meta_hash {
                note["metaHash"] = meta_hash.clone().into();
            }
            ids.insert(source_id, model_id.into());
        }
        if let Some((previous, identities)) = previous {
            for (old, model_id) in previous["annotations"].as_array().into_iter().flatten().zip(identities) {
                if model_id.is_empty() || model_ids.contains(model_id) {
                    continue;
                }
                let mut tombstone = old.clone();
                tombstone["_bokheim_id"] = model_id.clone().into();
                tombstone["deletedAt"] = exported_at.into();
                tombstone["updatedAt"] = exported_at.into();
                if let Some(id) = old["id"].as_str() {
                    ids.insert(id.to_owned(), model_id.clone().into());
                }
                annotations.push(tombstone);
            }
        }
        if let Some(retained) = object.remove(RETAINED) {
            let Value::Array(retained) = retained else {
                return Err(failure(Kind::InvalidData, "invalid retained Readest notes"));
            };
            // A Bokheim tombstone replaces an old active highlight; other
            // reader note types and native tombstones retain their contents.
            let retained = retained.into_iter().filter(|old| !annotations_id(&annotations, &old["id"])).collect::<Vec<_>>();
            annotations.extend(retained);
        }
        for key in ["$format", "version", "exportedAt", "book"] {
            object.remove(key);
        }
        ids.retain(|id, _| annotations.iter().any(|note| note["id"].as_str() == Some(id)));
        object.insert("booknotes".into(), annotations.into());
        object.insert("_bokheim_annotation_ids".into(), ids.into());
        object.insert("bookHash".into(), hash.clone().into());
        if let Some(meta_hash) = meta_hash {
            object.insert("metaHash".into(), meta_hash.clone().into());
        }
        object.entry("schemaVersion").or_insert(3.into());
        let updated = object.get("updatedAt").and_then(Value::as_u64).unwrap_or(0).saturating_add(1).max(exported_at);
        object.insert("updatedAt".into(), updated.into());
        serde_json::to_vec_pretty(&document).map_err(input)
    }
}

fn annotations_id(notes: &[Value], id: &Value) -> bool {
    !id.is_null() && notes.iter().any(|note| &note["id"] == id)
}

// Skip metadata and other large index fields while discovering book hashes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadestIndexBook {
    hash: String,
    format: String,
    #[serde(default)]
    meta_hash: Option<String>,
    #[serde(default)]
    deleted_at: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ReaderProfiles {
    pub(crate) foliate: Vec<PathBuf>,
    readest: HashMap<String, Vec<ProfileTarget>>,
    readest_error: Option<(Kind, String)>,
}

impl ReaderProfiles {
    pub(crate) fn discover(formats: &[AnnotationExportFormat]) -> io::Result<Self> {
        let mut foliate = foliate_directories()?;
        let mut readest = readest_directories()?;
        if !formats.contains(&AnnotationExportFormat::Foliate) {
            foliate.clear();
        }
        if !formats.contains(&AnnotationExportFormat::Readest) {
            readest.clear();
        }
        Self::from_directories(foliate, readest)
    }

    pub(crate) fn from_directories(foliate: Vec<PathBuf>, readest: Vec<(PathBuf, PathBuf)>) -> io::Result<Self> {
        let mut profiles = Self::default();
        let mut foliate_seen = HashSet::new();
        for root in foliate.into_iter().filter(|root| root.is_dir()) {
            let root = fs::canonicalize(root)?;
            if foliate_seen.insert(root.clone()) {
                profiles.foliate.push(root);
            }
        }
        let mut seen = HashSet::new();
        for (data, config) in readest {
            let result = (|| -> io::Result<()> {
                let settings = crate::native::read_destination(&config.join("settings.json"))?;
                let books = readest_books_directory(&data, settings.as_deref()).map_err(input)?;
                let Some(bytes) = crate::native::read_destination(&books.join("library.json"))? else { return Ok(()) };
                let books = fs::canonicalize(books)?;
                if !seen.insert(books.clone()) {
                    return Ok(());
                }
                let entries: Vec<ReadestIndexBook> = serde_json::from_slice(&bytes).map_err(input)?;
                for book in entries.into_iter().filter(|book| book.deleted_at.is_none() && book.format == "EPUB") {
                    let hash = book.hash;
                    if hash.len() != 32 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                        return Err(failure(Kind::InvalidData, "invalid Readest book hash"));
                    }
                    let target = ProfileTarget::Readest { path: books.join(&hash).join("config.json"), hash: hash.clone(), meta_hash: book.meta_hash };
                    profiles.readest.entry(hash).or_default().push(target);
                }
                Ok(())
            })();
            if let Err(error) = result {
                profiles.readest_error = Some((failure_details(&error).kind, error.to_string()));
            }
        }
        Ok(profiles)
    }

    pub(crate) fn targets(&self, format: AnnotationExportFormat, identity: &str) -> io::Result<Vec<ProfileTarget>> {
        match format {
            AnnotationExportFormat::Foliate => {
                if identity.is_empty() {
                    return Err(failure(Kind::Unavailable, "the local EPUB is needed to identify its Foliate book data"));
                }
                let name = format!("{}.json", encoded_identifier(identity));
                let roots = self.foliate.iter().collect::<Vec<_>>();
                if roots.is_empty() {
                    return Err(failure(Kind::Unavailable, "Foliate has no local profile; open Foliate once and retry"));
                }
                Ok(roots.into_iter().map(|root| ProfileTarget::Foliate { path: root.join(&name), identifier: identity.to_owned() }).collect())
            }
            AnnotationExportFormat::Readest => {
                if let Some((kind, message)) = &self.readest_error {
                    return Err(failure(*kind, message));
                }
                self.readest.get(identity).cloned().ok_or_else(|| failure(Kind::Unavailable, "this EPUB is not in a local Readest library; add the same book to Readest and retry"))
            }
            _ => unreachable!("only reader formats have profiles"),
        }
    }
}

/// Readest's partialMD5 samples 1 KiB at zero and powers of four KiB.
/// JavaScript's initial `1024 << -2` wraps to zero, not 256 bytes.
pub(crate) fn readest_hash(mut reader: impl Read + Seek) -> io::Result<String> {
    let size = reader.seek(SeekFrom::End(0))?;
    let mut digest = Md5::new();
    for offset in std::iter::once(0).chain((0..=10).map(|i| 1024u64 << (2 * i))) {
        if offset >= size {
            break;
        }
        reader.seek(SeekFrom::Start(offset))?;
        let mut buffer = [0; 1024];
        let count = (size - offset).min(buffer.len() as u64) as usize;
        reader.read_exact(&mut buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_inspection::InspectedBook;
    use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookDetails, BookFormat, BookRecord};
    use content_address::{BookId, ContentHash};
    use library_database::{Database, ImportCommit};
    use library_files::AssetStore;
    use library_model::{AnnotationExportConflict, AnnotationExportOptions, AnnotationExportOutcome, PublishedStagedBook};
    use std::io::Cursor;
    use sync_common::{LibraryId, ROOT_DIR_ID};

    #[test]
    fn readest_partial_hash_matches_upstream_sample_boundaries() {
        for (length, expected) in
            [(0, "d41d8cd98f00b204e9800998ecf8427e"), (1023, "7437d7a881387db7c41cf6930ab5e35d"), (2048, "1544ef7a46131a30cf328f55979db73d"), (6144, "5b0d78ea442dac69d258c776cc8d0155"), (2_097_152, "da96d9df9e3ed2a4e922f3641ba40600")]
        {
            let bytes = (0..length).map(|index| (index % 251) as u8).collect::<Vec<_>>();
            assert_eq!(readest_hash(Cursor::new(bytes)).unwrap(), expected);
        }
    }

    #[test]
    fn discovery_uses_the_configured_readest_directory_and_matching_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("native-data");
        let config = temp.path().join("native-config");
        let books = temp.path().join("custom-books");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&books).unwrap();
        let hash = "0123456789abcdef0123456789abcdef";
        fs::write(config.join("settings.json"), json!({"localBooksDir":books}).to_string()).unwrap();
        fs::write(books.join("library.json"), json!([{"hash":hash,"format":"EPUB","metaHash":"metadata"}]).to_string()).unwrap();
        let profiles = ReaderProfiles::from_directories(vec![], vec![(data, config)]).unwrap();
        let targets = profiles.targets(AnnotationExportFormat::Readest, hash).unwrap();
        assert_eq!(targets[0].path(), books.join(hash).join("config.json"));
        assert!(profiles.targets(AnnotationExportFormat::Readest, "another hash").is_err());
        assert_eq!(encoded_identifier("urn:uuid:a b/å"), "urn%3Auuid%3Aa%20b%2F%C3%A5");
    }

    #[test]
    fn native_config_validation_rejects_identity_mismatches() {
        let target = ProfileTarget::Readest { path: "/reader/config.json".into(), hash: "expected".into(), meta_hash: None };
        let mut expected = json!({"book":{"title":"Book","author":""}});
        let error = target.decode(br#"{"bookHash":"another","booknotes":[]}"#, &mut expected).unwrap_err();
        assert_eq!(failure_details(&error).kind, Kind::Identity);
        assert!(target.decode(br#"{"booknotes":[null]}"#, &mut expected).is_err());
        let target = ProfileTarget::Foliate { path: "/reader/book.json".into(), identifier: "expected".into() };
        assert!(target.decode(br#"{"metadata":{"identifier":"another"}}"#, &mut expected).is_err());
        let adapted = target.decode(br#"{"metadata":{"identifier":"expected"},"_bokheim_book_id":"another-library"}"#, &mut expected).unwrap();
        let adapted: Value = serde_json::from_slice(&adapted).unwrap();
        assert_eq!(adapted["annotations"], json!([]));
        assert!(adapted.get("_bokheim_book_id").is_none());
    }

    #[test]
    fn new_readest_ids_keep_the_library_annotation_identity() {
        let target = ProfileTarget::Readest { path: "/reader/config.json".into(), hash: "hash".into(), meta_hash: None };
        let bytes = serde_json::to_vec(&json!({"annotations":[{"id":"model","type":"annotation","cfi":"epubcfi(/6/2!/4/2/1:0)"}],"_bokheim_annotation_ids":{"model":"model"}})).unwrap();
        let encoded = target.encode(bytes, None, 10).unwrap();
        let config: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(config["booknotes"][0]["id"], "model");
        assert_eq!(config["booknotes"][0]["_bokheim_id"], "model");
    }

    #[test]
    fn readest_deletions_and_restoration_keep_the_native_id() {
        let target = ProfileTarget::Readest { path: "/reader/config.json".into(), hash: "hash".into(), meta_hash: None };
        let previous = json!({"annotations":[{"id":"native","type":"annotation","cfi":"epubcfi(/6/2!/4/2/1:0)","createdAt":1}],"_bokheim_annotation_ids":{"native":"model"}});
        let removed = target.encode(serde_json::to_vec(&json!({"annotations":[],"_bokheim_annotation_ids":{"native":"model"}})).unwrap(), Some((&previous, &["model".into()])), 10).unwrap();
        let removed: Value = serde_json::from_slice(&removed).unwrap();
        assert_eq!(removed["booknotes"][0]["id"], "native");
        assert_eq!(removed["booknotes"][0]["deletedAt"], 10);
        assert_eq!(removed["_bokheim_annotation_ids"]["native"], "model");
        let mut expected = json!({"book":{"title":"Book","author":""}});
        let previous = target.decode(&serde_json::to_vec(&removed).unwrap(), &mut expected).unwrap();
        let previous: Value = serde_json::from_slice(&previous).unwrap();
        let mut restored = previous.clone();
        restored["_bokheim_annotation_ids"] = json!({"model":"model"});
        restored["annotations"] = json!([{"id":"model","type":"annotation","cfi":"epubcfi(/6/2!/4/2/1:0)"}]);
        let restored = target.encode(serde_json::to_vec(&restored).unwrap(), Some((&previous, &[])), 20).unwrap();
        let restored: Value = serde_json::from_slice(&restored).unwrap();
        assert_eq!(restored["booknotes"].as_array().unwrap().len(), 1);
        assert_eq!(restored["booknotes"][0]["id"], "native");
        assert!(restored["booknotes"][0]["deletedAt"].is_null());
    }

    #[tokio::test]
    async fn installs_reader_book_data_preserves_external_notes_and_repeats_without_duplicates() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let foliate_root = temp.path().join("foliate");
        let readest_root = temp.path().join("readest");
        let readest_books = readest_root.join("Readest/Books");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&foliate_root).unwrap();
        fs::create_dir_all(&readest_books).unwrap();
        let bytes = include_bytes!("../../../tests/fixtures/cloud-cover.epub");
        fs::write(root.join("Book.epub"), bytes).unwrap();
        let publication = book_content::Book::open_reader(book_content::BookFormat::Epub, Cursor::new(bytes.to_vec())).unwrap();
        let identifier = publication.publication_identifier().unwrap();
        let hash = readest_hash(Cursor::new(bytes)).unwrap();
        let foliate_path = foliate_root.join(format!("{}.json", encoded_identifier(&identifier)));
        let readest_path = readest_books.join(&hash).join("config.json");
        fs::create_dir_all(readest_path.parent().unwrap()).unwrap();
        fs::write(readest_books.join("library.json"), json!([{"hash":hash,"format":"EPUB"}]).to_string()).unwrap();
        let foliate_note = json!({"value":"epubcfi(/6/2!/4/2/1:0)","text":"From Foliate","note":"Foreign note","color":"yellow","created":"2026-01-01T00:00:00Z","modified":"2026-01-01T00:00:00Z"});
        fs::write(&foliate_path, json!({"metadata":{"identifier":identifier},"progress":[3,10],"lastLocation":"foliate position","bookmarks":["bookmark"],"annotations":[foliate_note]}).to_string()).unwrap();
        let foreign = json!({"id":"reader-note","type":"annotation","cfi":"epubcfi(/6/2!/4/2/1:1)","text":"From Readest","note":"Reader note","style":"highlight","color":"green","createdAt":10,"updatedAt":11});
        let retained = vec![json!({"id":"bookmark","type":"bookmark","cfi":"epubcfi(/6/2!/4/2/1:2)"}), json!({"id":"ink","type":"ink","strokes":[1,2]}), json!({"id":"deleted","type":"annotation","deletedAt":5})];
        let mut notes = retained.clone();
        notes.push(foreign.clone());
        notes.push(json!({"id":"foliate-note","type":"annotation","cfi":"epubcfi(/6/2!/4/2/1:0)","text":"From Foliate","note":"Foreign note","color":"yellow","createdAt":1,"updatedAt":2}));
        fs::write(&readest_path, json!({"schemaVersion":3,"updatedAt":1,"progress":[7,20],"location":"readest position","viewSettings":{"fontSize":19},"booknotes":notes}).to_string()).unwrap();
        let library = LibraryId::new_v4();
        let checksum = ContentHash::from(blake3::hash(bytes));
        let book = BookId::for_local_content(checksum);
        let database = Database::open(temp.path().join("library.sqlite3")).unwrap();
        database.initialize_library_for(library).await.unwrap();
        database
            .commit_import(ImportCommit {
                source_identity: None,
                byte_checksum: Some(checksum),
                audiobook_content_hash: None,
                parent_id: ROOT_DIR_ID,
                file_name: "Book.epub".into(),
                format: BookFormat::Epub,
                book_id: book,
                size_bytes: bytes.len() as u64,
                inspection_version: 1,
                inspection: Some(InspectedBook { metadata: BookRecord { metadata: BookDetails { title: "Book".into(), ..Default::default() }, description: String::new() }, pdf: None, audiobook: None, toc: vec![] }),
                published: PublishedStagedBook { name: "Book.epub".into(), relative_path: "/Book.epub".into(), published: true, fingerprint: Some("epub".into()) },
                request_thumbnail: false,
                restore_paths: vec![],
            })
            .await
            .unwrap();
        database
            .upsert_annotation(&Annotation {
                id: "bokheim-note".into(),
                book_id: book,
                anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:3)"),
                exact_text: "From Bokheim".into(),
                note: "Own note".into(),
                style: AnnotationStyle::Strikethrough,
                color: 0xffcc00,
                created_at: 20,
                modified_at: 21,
                toc_ordinal: None,
                progress: None,
                deleted: false,
            })
            .await
            .unwrap();
        let assets = AssetStore::open(root.to_str().unwrap()).unwrap();
        let output = temp.path().join("exports");
        let options = AnnotationExportOptions {
            destination: output.clone(),
            formats: vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest],
            install_readers: true,
            conflict: AnnotationExportConflict::Replace,
            overrides: vec![],
        };
        let before = database.sync_outbox_watermark().await.unwrap();
        for _ in 0..2 {
            let catalog = ReaderProfiles::from_directories(vec![foliate_root.clone()], vec![(readest_root.clone(), temp.path().join("config"))]).unwrap();
            let mut books = database.annotation_export_snapshot().await.unwrap();
            let plan = crate::plan(library, &books, &options).unwrap();
            let report = crate::native::export_with_profiles(&database, &assets, library, options.clone(), &mut books, plan, Some(catalog)).await.unwrap();
            assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
            assert!(report.results.iter().any(|result| result.destination.path == foliate_path));
            assert!(report.results.iter().any(|result| result.destination.path == readest_path));
            assert_eq!(database.annotation_history(book).unwrap().len(), 1);
            let foliate: Value = serde_json::from_slice(&fs::read(&foliate_path).unwrap()).unwrap();
            assert_eq!(foliate["annotations"].as_array().unwrap().len(), 2);
            assert!(foliate["annotations"].as_array().unwrap().contains(&foliate_note));
            assert_eq!(foliate["progress"], json!([3, 10]));
            assert_eq!(foliate["lastLocation"], "foliate position");
            assert_eq!(foliate["bookmarks"], json!(["bookmark"]));
            let readest: Value = serde_json::from_slice(&fs::read(&readest_path).unwrap()).unwrap();
            let notes = readest["booknotes"].as_array().unwrap();
            assert_eq!(notes.len(), 6);
            for item in &retained {
                assert!(notes.contains(item));
            }
            assert!(notes.contains(&foreign));
            assert_eq!(notes.iter().find(|note| readest["_bokheim_annotation_ids"][note["id"].as_str().unwrap()] == "bokheim-note").unwrap()["style"], "underline");
            assert_eq!(readest["progress"], json!([7, 20]));
            assert_eq!(readest["location"], "readest position");
            assert_eq!(readest["viewSettings"]["fontSize"], 19);
            assert_eq!(readest["bookHash"], hash);
            assert!(readest.get("$format").is_none());
            assert!(readest.get(RETAINED).is_none());
            assert_eq!(fs::read_dir(&output).unwrap().count(), 1, "reader configs must not be written to the Markdown folder");
            let mut rewritten = readest;
            rewritten.as_object_mut().unwrap().remove("_bokheim_annotation_ids");
            fs::write(&readest_path, serde_json::to_vec(&rewritten).unwrap()).unwrap();
        }
        assert_eq!(database.sync_outbox_watermark().unwrap(), before, "export must not import or queue external annotations for sync");
    }
}
