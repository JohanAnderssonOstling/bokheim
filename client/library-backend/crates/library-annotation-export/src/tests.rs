use super::*;
use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookDetails, BookFormat, BookTocEntry};
use content_address::BookId;
use sync_common::{LibraryId, ROOT_DIR_ID};

fn book() -> AnnotationExportBook {
    let id = BookId::from_bytes([5; 32]);
    AnnotationExportBook {
        book_id: id,
        format: BookFormat::Epub,
        metadata: BookDetails { title: "Title: \"quoted\"\n---".into(), ..Default::default() },
        toc: vec![BookTocEntry { title: "Part".into(), target: "a.xhtml".into(), children: vec![BookTocEntry { title: "Child".into(), target: "a.xhtml#child".into(), children: vec![] }] }],
        annotations: vec![Annotation {
            id: "one".into(),
            book_id: id,
            anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/8/12!/4/2,/1:0,/1:7)"),
            exact_text: "a quote\n\na second line".into(),
            style: AnnotationStyle::Underline,
            color: 0xabcdef,
            note: "my *note*".into(),
            created_at: 1,
            modified_at: 1234,
            toc_ordinal: Some(1),
            progress: Some(1.0),
            deleted: false,
        }],
        canonical_hash: Some([4; 32].into()),
        placements: vec![AnnotationExportPlacement { directory_id: ROOT_DIR_ID, relative_path: "/Folder/Original.epub".into(), downloaded: true, local_hash: Some([3; 32].into()) }],
    }
}

#[test]
fn markdown_frontmatter_preserves_metadata_without_breaking_yaml_and_keeps_toc_hierarchy() {
    let book = book();
    let library = LibraryId::new_v4();
    let text = markdown(library, &book, &book.placements[0], 0).unwrap();
    let (_, front) = text.split_once("---\n").unwrap();
    let (front, _) = front.split_once("\n---\n").unwrap();
    let title = front.lines().find_map(|line| line.strip_prefix("title: ")).unwrap();
    assert_eq!(serde_json::from_str::<String>(title).unwrap(), book.metadata.title);
    assert!(text.contains("## Part\n\n### Child\n\n> *a quote*\n>\n> *a second line*\n\nmy *note*"));
    let annotation = &book.annotations[0];
    let source = library_model::ReaderLink { locator: library_model::BookLocator::new(library, book.book_id), annotation_id: Some(annotation.id.clone()), anchor: annotation.anchor.clone() };
    let label = annotation_format::annotation_source_label(&book.metadata.title, Some("Child"), &annotation.anchor);
    assert!(text.ends_with(&annotation_format::annotation_markdown(&annotation.exact_text, &annotation.note, &label, &source)));
    assert!(text.contains("exported_at: \"1970-01-01T00:00:00.000Z\""));
    assert!(text.contains(&format!("book_id: \"{}\"", book.book_id)));
}
#[test]
fn foliate_uses_full_cfis_and_interoperable_style_and_timestamp_fields() {
    let book = book();
    let bytes = foliate(&book, Some("urn:uuid:original-id")).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(data["metadata"]["identifier"], "urn:uuid:original-id");
    assert_eq!(data["annotations"][0]["value"], "epubcfi(/8/12!/4/2,/1:0,/1:7)");
    assert_eq!(data["annotations"][0]["color"], "underline");
    assert_eq!(data["annotations"][0]["modified"], "1970-01-01T00:00:01.234Z");
    assert_eq!(data["annotations"][0]["note"], "my *note*");
    assert_eq!(data["_bokheim_annotation_ids"]["epubcfi(/8/12!/4/2,/1:0,/1:7)"], "one");
    assert_eq!(data["annotations"][0].as_object().unwrap().len(), 6);
    assert!(data["annotations"][0].as_object().unwrap().keys().all(|key| matches!(key.as_str(), "value" | "color" | "text" | "note" | "created" | "modified")));
    assert!(data["annotations"][0].get("_bokheim").is_none());
}
#[test]
fn foliate_rejects_duplicate_locations_instead_of_losing_annotations_on_import() {
    let mut book = book();
    let mut duplicate = book.annotations[0].clone();
    duplicate.id = "another-note".into();
    book.annotations.push(duplicate);
    assert!(foliate(&book, Some("book-id")).is_err());
}

#[test]
fn readest_uses_the_versioned_import_envelope_and_preserves_annotation_fields() {
    let mut book = book();
    let original = book.annotations[0].clone();
    book.annotations.clear();
    for (index, style) in [AnnotationStyle::Highlight, AnnotationStyle::Underline, AnnotationStyle::Squiggly, AnnotationStyle::Strikethrough].into_iter().enumerate() {
        let mut annotation = original.clone();
        annotation.id = format!("note-{index}");
        annotation.style = style;
        book.annotations.push(annotation);
    }
    let mut deleted = original.clone();
    deleted.deleted = true;
    deleted.style = AnnotationStyle::Strikethrough;
    book.annotations.push(deleted);
    let bytes = readest(&book, 9876).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(data["$format"], "readest-annotations");
    assert_eq!(data["version"], 1);
    assert_eq!(data["exportedAt"], 9876);
    assert_eq!(data["book"]["title"], book.metadata.title);
    assert_eq!(data["book"]["format"], "EPUB");
    assert!(data["book"].get("hash").is_none(), "Bokheim hashes must not be labeled as Readest MD5s");
    assert!(data.get("location").is_none());
    let annotations = data["annotations"].as_array().unwrap();
    assert_eq!(annotations.len(), 4);
    for (index, style) in ["highlight", "underline", "squiggly", "underline"].into_iter().enumerate() {
        assert_eq!(
            annotations[index],
            serde_json::json!({
                "id": format!("note-{index}"), "type": "annotation", "cfi": "epubcfi(/8/12!/4/2,/1:0,/1:7)",
                "text": original.exact_text, "note": original.note, "color": "#abcdef", "style": style,
                "createdAt": 1, "updatedAt": 1234,
            })
        );
    }
    assert_eq!(readest(&book, 9876).unwrap(), bytes, "repeat exports retain annotation IDs");
}
#[test]
fn readest_rejects_unsupported_formats() {
    let mut book = book();
    book.format = BookFormat::Pdf;
    assert!(readest(&book, 0).unwrap_err().to_string().contains("EPUB"));
}
#[test]
fn readest_plans_title_based_files_for_epubs_in_their_library_folders() {
    let mut epub = book();
    epub.metadata.title = "A book".into();
    let mut pdf = epub.clone();
    pdf.book_id = BookId::from_bytes([6; 32]);
    pdf.format = BookFormat::Pdf;
    let options = AnnotationExportOptions { destination: PathBuf::from("/outside/Library"), formats: vec![AnnotationExportFormat::Readest], install_readers: false, conflict: AnnotationExportConflict::Replace, overrides: vec![] };
    let plan = plan(LibraryId::new_v4(), &[epub, pdf], &options).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].format, AnnotationExportFormat::Readest);
    assert_eq!(plan.items[0].path, Path::new("/outside/Library/Folder/A book.readest.json"));
}
#[test]
fn plans_mirror_library_folders_and_preserve_distinct_books_with_the_same_title() {
    let mut book = book();
    book.metadata.title = "Same Title".into();
    let mut other = book.clone();
    other.book_id = BookId::from_bytes([6; 32]);
    let options = AnnotationExportOptions {
        destination: PathBuf::from("/outside/Library"),
        formats: vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Pdf],
        install_readers: false,
        conflict: AnnotationExportConflict::KeepBoth,
        overrides: vec![],
    };
    let plan = plan(LibraryId::new_v4(), &[book.clone(), other], &options).unwrap();
    assert_eq!(plan.items.len(), 4);
    assert_eq!(plan.items[0].path, Path::new("/outside/Library/Folder/Same Title.md"));
    assert_eq!(plan.items[2].path, Path::new("/outside/Library/Folder/Same Title (2).md"));
    assert_eq!(plan.items[1].path, Path::new("/outside/Library/Folder/Same Title.foliate.json"));
    book.placements[0].relative_path = "/../escape.epub".into();
    assert!(super::plan(LibraryId::new_v4(), &[book], &options).is_err());
    assert_eq!(file_stem("../../Title: hi"), "_.._Title_ hi");
    assert_eq!(file_stem("CON"), "_CON");
}

#[test]
fn every_recurring_scope_exports_once_at_the_original_passage() {
    use book_model::RecurringScope;
    for scope in [RecurringScope::Chapter, RecurringScope::Book, RecurringScope::Library] {
        let mut book = book();
        let original = book.annotations[0].anchor.clone();
        book.annotations[0].anchor = original.with_recurring_scope(Some(scope));
        book.annotations[0].note = "Latest shared note".into();
        let markdown = markdown(LibraryId::new_v4(), &book, &book.placements[0], 0).unwrap();
        assert_eq!(markdown.matches("Latest shared note").count(), 1);
        assert!(markdown.contains("### Child"));
        for bytes in [foliate(&book, Some("original-id")).unwrap(), readest(&book, 0).unwrap()] {
            let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let notes = document["annotations"].as_array().unwrap();
            assert_eq!(notes.len(), 1);
            let note = &notes[0];
            assert_eq!(note["note"], "Latest shared note");
            assert_eq!(note.get("cfi").or_else(|| note.get("value")).unwrap(), original.epub_cfi_value().unwrap());
            assert!(note.get("_bokheim").is_none(), "merge history must stay out of reader exports");
            assert_eq!(note.get("id").unwrap_or(&document["_bokheim_annotation_ids"][original.epub_cfi_value().unwrap()]), &book.annotations[0].id);
            assert_eq!(book.annotations[0].anchor.recurring_scope(), Some(scope));
            assert_eq!(book.annotations[0].anchor.origin(), &original);
        }
    }
}
