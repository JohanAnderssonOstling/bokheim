use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookDetails, BookRecord, ContentHash, PdfAnnotationAnchor, PdfAnnotationRect};

#[test]
fn composed_book_record_preserves_its_historical_binary_and_json_formats() {
    let mut bytes = wire::header_bytes().to_vec();
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
        AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(3, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.4)], None)),
        AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(4, vec![], Some("epubcfi(/6/4)".into()))),
    ] {
        let annotation = Annotation {
            id: "note-1".into(),
            content_hash: ContentHash::new(&"a".repeat(64)),
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
        assert!(encoded.starts_with(&wire::header_bytes()));
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
