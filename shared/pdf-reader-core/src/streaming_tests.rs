fn annotated_streaming_fixture(rotation: u16) -> Vec<u8> {
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /CropBox [20 40 580 760] /Rotate {rotation} /Resources << >> /Annots [4 0 R 5 0 R 6 0 R 7 0 R] >>"),
        "<< /Type /Annot /Subtype /Highlight /Rect [60 600 240 632] /QuadPoints [60 632 240 632 60 600 240 600] /C [1 0 0] /Contents (Imported highlight) >>".to_owned(),
        "<< /Type /Annot /Subtype /Text /Rect [250 600 270 620] /Contents (Imported note) >>".to_owned(),
        "<< /Type /Annot /Subtype /Square /Rect [300 600 400 700] /Contents (Complex annotation) >>".to_owned(),
        "<< /Type /Annot /Subtype /Highlight /Rect [60 500 240 532] /NM (bokheim:owned) /Contents (App-owned mark) >>".to_owned(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    bytes
}

#[test]
fn rendering_keeps_simple_annotations_in_rotated_cropped_display_coordinates() -> PdfResult<()> {
    for rotation in [0, 90] {
        let bytes = annotated_streaming_fixture(rotation);
        let (_, metadata, _) = inspect_reader_metadata(Cursor::new(bytes.clone()))?;
        assert!(metadata.valid_for(&metadata.book_id));
        assert!(metadata.annotations.is_empty());
        let document = PdfDocumentSession::from_reader("annotations", Cursor::new(bytes))?;
        let rendered = document.render_page_for_display(PageRenderKey::new(0, 320))?;
        let annotations = rendered.embedded_annotations();
        assert_eq!(annotations.len(), 2, "complex and app-owned annotations must be omitted");
        assert_eq!(annotations[0].note, "Imported highlight");
        assert_eq!(annotations[1].kind, PdfStoredAnnotationKind::Note);
        assert_eq!(annotations[1].note, "Imported note");
        let rect = annotations[0].rects[0];
        let expected = if rotation == 0 { [40.0 / 560.0, 128.0 / 720.0, 180.0 / 560.0, 32.0 / 720.0] } else { [560.0 / 720.0, 40.0 / 560.0, 32.0 / 720.0, 180.0 / 560.0] };
        for (actual, expected) in rect.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.001, "rotation={rotation}, rect={rect:?}, actual={actual}, expected={expected}");
        }
    }
    Ok(())
}

#[test]
fn render_window_scales_with_settings_and_prioritizes_visible_pages() {
    let mut info = PdfDocumentInfo { source_name: "window".into(), pages: vec![PdfPageInfo { width_points: 600.0, height_points: 800.0 }; 30], outline: vec![], right_to_left: false };
    assert_eq!(info.render_window(10, &[10], 1), vec![10, 11]);
    assert_eq!(info.render_window(10, &[10, 11], 2), vec![10, 11, 12, 9]);
    assert_eq!(info.render_window(10, &[10, 11, 12, 13], 4), vec![10, 11, 12, 13, 14, 15, 16, 9]);
    assert_eq!(info.render_window(10, &[10, 11, 12, 13, 14, 15], 6).len(), 12);
    assert_eq!(info.render_window(0, &[0, 1, 2, 3], 4), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(info.render_window(29, &[29], 4), vec![29, 28, 27, 26, 25, 24, 23, 22]);
    assert!(info.render_window(30, &[], 4).is_empty());
    info.pages.truncate(3);
    assert_eq!(info.render_window(0, &[0], 4), vec![0, 1, 2]);
    info.pages.clear();
    assert!(info.render_window(0, &[], 4).is_empty());
}
