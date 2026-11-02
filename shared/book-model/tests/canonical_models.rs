use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookDetails, BookId, BookRecord, PdfAnnotationAnchor, PdfAnnotationRect};

#[test]
fn reader_annotation_colors_preserve_opaque_values_and_reject_invalid_components() {
    use book_model::parse_annotation_color;
    for (source, expected) in [("cyan", 0x00ffff), ("fuchsia", 0xff00ff), ("#1a2", 0x11aa22), ("#123abc", 0x123abc), ("rgb(1, 2, 3)", 0x010203), ("rgba(1,2,3,1)", 0x010203)] {
        assert_eq!(parse_annotation_color(source).unwrap(), Some(expected));
    }
    for source in ["#xyzxyz", "rgb(256,0,0)", "rgb(-1,0,0)"] {
        assert!(parse_annotation_color(source).is_err(), "{source}");
    }
    for source in ["#1234567", "rgba(1,2,3,0.5)", "unknown"] {
        assert_eq!(parse_annotation_color(source).unwrap(), None, "{source}");
    }
}

#[test]
fn composed_book_record_preserves_its_historical_binary_and_json_formats() {
    let mut bytes = wire::encode(&()).unwrap();
    bytes.extend_from_slice(b"\x06Legacy\x01\x08Subtitle\x00\x0bDescription\x00\x00\x00\x00\x00");
    let record: BookRecord = wire::decode(&bytes, 8192).unwrap();
    assert_eq!(record.metadata, BookDetails { title: "Legacy".into(), subtitle: Some("Subtitle".into()), contributors: vec![], book: Default::default() });
    assert_eq!(record.description, "Description");
    assert_eq!(wire::encode(&record).unwrap(), bytes);
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["title"], "Legacy");
    assert_eq!(json["description"], "Description");
    assert!(json.get("metadata").is_none());
    assert_eq!(serde_json::from_value::<BookRecord>(json).unwrap(), record);
}

#[test]
fn annotations_round_trip_their_binary_form() {
    for anchor in [
        AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
        AnnotationAnchor::epub_publication_cfi("epubcfi(/8[spine]/6[item]!/4/2/1:3)"),
        AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(3, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.4)], None)),
        AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(4, vec![], Some("epubcfi(/6/4)".into()))),
    ] {
        let annotation = Annotation {
            id: "note-1".into(),
            content_hash: BookId::new(&"a".repeat(64)),
            anchor,
            exact_text: "Text".into(),
            style: AnnotationStyle::Underline,
            color: 0xffffff,
            note: "note".into(),
            created_at: 7,
            toc_ordinal: Some(5),
            progress: Some(42.5),
            modified_at: 9,
            deleted: false,
        };
        let encoded = annotation.encode().unwrap();
        assert!(encoded.starts_with(&wire::encode(&()).unwrap()));
        assert_eq!(Annotation::decode(&encoded).unwrap(), annotation);
        let mut trailing = encoded;
        trailing.push(0);
        assert!(Annotation::decode(&trailing).is_err());
    }
    assert!(Annotation::decode(include_bytes!("fixtures/annotation_epub_detail.json")).is_err());
}

#[test]
fn book_publishers_are_plain_names_in_binary_and_json() {
    let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
    record.book.publishers = vec!["Oxford University Press".into(), "Éditions Gallimard".into()];
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["book"]["publishers"], serde_json::json!(["Oxford University Press", "Éditions Gallimard"]));
    assert_eq!(serde_json::from_value::<BookRecord>(json).unwrap(), record);
    let binary = wire::encode(&record).unwrap();
    assert_eq!(wire::decode::<BookRecord>(&binary, 8192).unwrap(), record);
}

#[test]
fn book_year_is_an_optional_integer_in_binary_and_json() {
    for year in [Some(2004), None] {
        let mut record = BookRecord { metadata: BookDetails::default(), description: String::new() };
        record.book.year = year;
        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["book"]["year"], serde_json::json!(year));
        assert!(json["book"].get("dates").is_none());
        assert_eq!(serde_json::from_value::<BookRecord>(json).unwrap(), record);
        let binary = wire::encode(&record).unwrap();
        assert_eq!(wire::decode::<BookRecord>(&binary, 8192).unwrap(), record);
    }
}

#[test]
fn recurring_scopes_keep_creation_anchors_and_round_trip_storage_and_sync() {
    use book_model::RecurringScope;
    let original = AnnotationAnchor::epub_publication_cfi("epubcfi(/8/4!/4/2,/1:0,/1:9)");
    let mut annotation = Annotation {
        id: "recurring-name".into(),
        content_hash: BookId::from_bytes([3; 32]),
        anchor: original.clone(),
        exact_text: "Elizabeth".into(),
        style: AnnotationStyle::Underline,
        color: 0xabcdef,
        note: "A shared character note".into(),
        created_at: 1,
        toc_ordinal: Some(7),
        progress: Some(24.0),
        modified_at: 2,
        deleted: false,
    };
    for scope in [RecurringScope::Chapter, RecurringScope::Book, RecurringScope::Library] {
        annotation.anchor = annotation.anchor.with_recurring_scope(Some(scope));
        annotation.validate().unwrap();
        assert_eq!(annotation.anchor.origin(), &original);
        assert_eq!(annotation.anchor.epub_cfi_value(), original.epub_cfi_value());
        assert!(annotation.anchor.is_epub_publication_cfi());
        assert_eq!(Annotation::decode(&annotation.encode().unwrap()).unwrap(), annotation);
        let transport = protobuf_wire::encode(&annotation).unwrap();
        assert_eq!(protobuf_wire::decode::<Annotation>(&transport, 65536).unwrap(), annotation);
        assert_eq!(AnnotationAnchor::from_open_target(&annotation.anchor.open_target().unwrap()).unwrap(), original);
        assert_eq!(annotation.toc_ordinal, Some(7));
    }
    annotation.anchor = annotation.anchor.with_recurring_scope(None);
    assert_eq!(annotation.anchor, original);
}
