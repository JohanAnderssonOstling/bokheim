use super::*;

#[cfg(not(target_os = "android"))]
#[test]
fn pdf_scanner_refreshes_changed_annotations_without_backfilling_unchanged_files() {
    use library_model::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation};
    let temp = tempdir().unwrap(); let root = temp.path().join("library"); fs::create_dir_all(&root).unwrap();
    let path = root.join("notes.pdf"); test_support::write_annotated_pdf(&path);

    let (state, hash_rx, _events) = test_scanner_state(&root);
    let worker_state = state.clone(); let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
    LibraryScanner::scan_library(&state.db, &state);
    let hash = publication_identity::identify(&mut fs::File::open(&path).unwrap()).unwrap();
    let runtime = crate::executor::BackendExecutor::new().unwrap();
    let opened = || runtime.block_on(state.assets.prepare_pdf_reader(hash)).unwrap().unwrap().1;
    let original = opened().expect("scanned file has matching metadata");
    assert_eq!(original.annotations.len(), 1);
    {
        let conn = state.db.get().unwrap();
        client_library::marks::store_annotation(&conn, &ReaderAnnotation { id: "user-note".into(), content_hash: hash,
            anchor: AnnotationAnchor::pdf(PdfAnnotationAnchor::new(0, vec![PdfAnnotationRect::new(0.1, 0.1, 0.2, 0.03)], None)),
            exact_text: "User selection".into(), style: AnnotationStyle::Underline, color: "#abcdef".into(), note: "Keep my note".into(), created_at: 1, modified_at: 1 }).unwrap();
        conn.execute(include_str!("../sql/pdf_streaming_tests/pdf_scanner_refreshes_changed_annotations_without_backfilling_unchanged_files_delete.sql"), [hash.as_str()]).unwrap();
    }
    LibraryScanner::scan_library(&state.db, &state);
    assert!(opened().is_none(), "unchanged PDFs must not be backfilled, even when their description is missing");
    {
        let mut conn = state.db.get().unwrap(); let tx = conn.transaction().unwrap();
        client_library::pdf_metadata::upsert(&tx, &hash, &original.checksum.parse().unwrap(), &original).unwrap();
        tx.commit().unwrap();
    }
    assert!(opened().is_some());
    let mut document = lopdf::Document::load(&path).unwrap();
    let page = *document.get_pages().get(&1).unwrap();
    let annotation = document.get_object(page).unwrap().as_dict().unwrap().get(b"Annots").unwrap().as_array().unwrap()[0].as_reference().unwrap();
    document.get_object_mut(annotation).unwrap().as_dict_mut().unwrap().set("Contents", lopdf::Object::string_literal("Updated external note"));
    document.save(&path).unwrap();
    assert!(opened().is_none(), "a changed file cannot reuse stale metadata before scanning");
    LibraryScanner::scan_library(&state.db, &state);
    let changed = opened().expect("changed file is inspected normally");
    assert_ne!(changed.checksum, original.checksum);
    assert_eq!(changed.annotations.len(), 1); assert_eq!(changed.annotations[0].note, "Updated external note");
    let stored = client_library::marks::load_annotations(&state.db.get().unwrap(), hash).unwrap();
    assert_eq!(stored.len(), 1); assert_eq!(stored[0].note, "Keep my note");
    state.should_finish.store(true, Ordering::Release); worker.join().unwrap();
}
