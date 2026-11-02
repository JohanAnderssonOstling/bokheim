//! Native-profile integration check. A child process isolates HOME/XDG paths;
//! renaming its /proc comm exercises the real running-reader detection.
use super::*;
use crate::library::handle::concurrency_tests::fixture;
use book_model::{Annotation, AnnotationAnchor, AnnotationStyle};
use library_model::AutomaticAnnotationExportState;
use serde_json::{json, Value};
use std::{env, path::PathBuf, process::Command};
use sync_common::ROOT_DIR_ID;

const CHILD_HOME: &str = "BOKHEIM_READER_LIFECYCLE_HOME";
const HASH: &str = "5104e9fdfa29deb9464c2ad8028d329d";
const IDENTIFIER: &str = "cloud-cover-fixture";

fn document(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn active_readest_notes(document: &Value) -> Vec<&Value> {
    document["booknotes"].as_array().unwrap().iter().filter(|note| note["type"] == "annotation" && note["deletedAt"].is_null()).collect()
}

#[tokio::test]
#[ignore = "requires Foliate and Readest closed; exercises native process detection in an isolated child"]
async fn native_reader_lifecycle() {
    let formats = vec![AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest];
    let Some(home) = env::var_os(CHILD_HOME).map(PathBuf::from) else {
        assert!(library_annotation_export::running_readers(&formats).is_empty(), "close Foliate and Readest before running this manual integration check");
        let home = tempfile::tempdir().unwrap();
        let module = module_path!().split_once("::").unwrap().1;
        let result = Command::new(env::current_exe().unwrap())
            .args(["--exact", &format!("{module}::native_reader_lifecycle"), "--ignored", "--nocapture"])
            .env(CHILD_HOME, home.path())
            .env("HOME", home.path())
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .status()
            .unwrap();
        assert!(result.success());
        assert!(home.path().join("verified").exists(), "the child must execute the complete lifecycle");
        return;
    };

    let root = home.join("library");
    fs::create_dir_all(&root).unwrap();
    let (actor, _, _) = fixture(&root, "http://127.0.0.1:1".parse().unwrap());
    let session = &*actor.session;
    session.db.set_asset_storage_policy(false, false).unwrap();
    session.sync.set_asset_storage_enabled(false);
    let source = include_bytes!("../../../tests/fixtures/cloud-cover.epub");
    let book = session.import.import_downloaded_book(ROOT_DIR_ID, "Book.epub", source.to_vec()).await.unwrap();
    let mut annotation = Annotation {
        id: "lifecycle-note".into(),
        content_hash: book,
        anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:5)"),
        exact_text: "Quote".into(),
        note: "First note".into(),
        style: AnnotationStyle::Highlight,
        color: 0xffcc00,
        created_at: 1,
        modified_at: 2,
        deleted: false,
        toc_ordinal: None,
        progress: None,
    };
    session.db.upsert_annotation(&annotation).unwrap();
    let foliate = home.join(format!("data/com.github.johnfactotum.Foliate/{IDENTIFIER}.json"));
    let readest_books = home.join("data/com.bilingify.readest/Readest/Books");
    let readest = readest_books.join(HASH).join("config.json");
    fs::create_dir_all(foliate.parent().unwrap()).unwrap();
    fs::create_dir_all(readest.parent().unwrap()).unwrap();
    fs::write(readest_books.join("library.json"), json!([{"hash": HASH, "format":"EPUB"}]).to_string()).unwrap();
    fs::write(&foliate, json!({"metadata":{"identifier":IDENTIFIER},"annotations":[],"progress":[3,10],"lastLocation":"foliate position","bookmarks":["bookmark"]}).to_string()).unwrap();
    let bookmark = json!({"id":"bookmark","type":"bookmark","cfi":"epubcfi(/6/2!/4/2/1:0)"});
    fs::write(&readest, json!({"schemaVersion":3,"booknotes":[bookmark],"progress":[4,10],"location":"readest position","viewSettings":{"fontSize":19}}).to_string()).unwrap();
    let foliate_before = fs::read(&foliate).unwrap();
    session
        .set_automatic_annotation_export(Some(AnnotationExportOptions {
            destination: home.join("exports"),
            formats: vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest],
            install_readers: true,
            conflict: AnnotationExportConflict::Replace,
            overrides: vec![],
        }))
        .unwrap();
    let receipts_path = home.join("receipts.json");
    let mut receipts = Vec::new();

    // Foliate waits, while Markdown and the selected, closed Readest proceed.
    fs::write("/proc/self/comm", "foliate").unwrap();
    assert_eq!(library_annotation_export::running_readers(&formats), vec![AnnotationExportFormat::Foliate]);
    assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
    let status = session.automatic_annotation_export().unwrap();
    assert_eq!(status.outputs.len(), 1);
    assert_eq!(status.outputs[0].state, AutomaticAnnotationExportState::Waiting);
    assert_eq!(status.outputs[0].destination.format, AnnotationExportFormat::Foliate);
    assert_eq!(status.outputs[0].failure.kind, AnnotationExportFailureKind::Busy);
    assert_eq!(fs::read(&foliate).unwrap(), foliate_before);
    assert_eq!(active_readest_notes(&document(&readest)).len(), 1);
    let markdown = receipts.iter().find(|receipt| receipt.destination.format == AnnotationExportFormat::Markdown).unwrap().destination.path.clone();
    annotation.note = "Edited while Foliate is open".into();
    annotation.modified_at += 1;
    session.db.upsert_annotation(&annotation).unwrap();
    assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
    assert!(fs::read_to_string(&markdown).unwrap().contains(&annotation.note));
    let readest_before = fs::read(&readest).unwrap();

    // Closing Foliate publishes the queued latest version, without rewriting Readest.
    fs::write("/proc/self/comm", "bokheim-test").unwrap();
    assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
    assert!(session.automatic_annotation_export().unwrap().outputs.is_empty());
    assert_eq!(document(&foliate)["annotations"][0]["note"], annotation.note);
    assert_eq!(fs::read(&readest).unwrap(), readest_before);
    assert_eq!(receipts.len(), 3);

    // Discover a Foliate note while Readest is open; it must enter ordinary sync state.
    let mut existing = document(&foliate);
    existing["annotations"].as_array_mut().unwrap().push(json!({
        "value":"epubcfi(/6/2!/4/4,/1:0,/1:4)","text":"Foreign quote","note":"Reader-created note","color":"yellow",
        "created":"2026-01-01T00:00:00Z","modified":"2026-01-01T00:00:00Z",
    }));
    fs::write(&foliate, serde_json::to_vec(&existing).unwrap()).unwrap();
    annotation.color = 0x123456;
    annotation.modified_at += 1;
    session.db.upsert_annotation(&annotation).unwrap();
    let watermark = session.db.sync_outbox_watermark().unwrap();
    fs::write("/proc/self/comm", "readest").unwrap();
    assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
    let status = session.automatic_annotation_export().unwrap();
    assert_eq!(status.outputs.len(), 1);
    assert_eq!(status.outputs[0].destination.format, AnnotationExportFormat::Readest);
    assert_eq!(status.outputs[0].failure.kind, AnnotationExportFailureKind::Busy);
    assert_eq!(fs::read(&readest).unwrap(), readest_before);
    assert_eq!(session.db.annotations(book).unwrap().len(), 2);
    assert!(session.db.sync_outbox_watermark().unwrap() > watermark);
    assert!(fs::read_to_string(&markdown).unwrap().contains("Reader-created note"));
    fs::write("/proc/self/comm", "bokheim-test").unwrap();
    assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
    assert_eq!(active_readest_notes(&document(&readest)).len(), 2);
    let stable = (fs::read(&foliate).unwrap(), fs::read(&readest).unwrap(), fs::read(&markdown).unwrap());
    assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
    assert_eq!((fs::read(&foliate).unwrap(), fs::read(&readest).unwrap(), fs::read(&markdown).unwrap()), stable, "settled exports must not repeat writes");

    session.db.delete_annotation(&annotation.id, 5).unwrap();
    assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
    assert_eq!(document(&foliate)["annotations"].as_array().unwrap().len(), 1);
    let config = document(&readest);
    assert_eq!(active_readest_notes(&config).len(), 1);
    assert!(config["booknotes"].as_array().unwrap().iter().any(|note| !note["deletedAt"].is_null()), "Readest receives a native deletion tombstone");
    assert!(config["booknotes"].as_array().unwrap().contains(&bookmark));
    assert_eq!(config["progress"], json!([4, 10]));
    assert_eq!(config["location"], "readest position");
    assert_eq!(config["viewSettings"]["fontSize"], 19);
    let config = document(&foliate);
    assert_eq!(config["progress"], json!([3, 10]));
    assert_eq!(config["lastLocation"], "foliate position");
    assert_eq!(config["bookmarks"], json!(["bookmark"]));
    assert_eq!(fs::read(root.join("Book.epub")).unwrap(), source);
    fs::write(home.join("verified"), "waiting, resumption, edits, deletions, discovery, sync, deduplication and progress verified").unwrap();
}
