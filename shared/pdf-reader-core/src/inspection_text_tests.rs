// Metadata extraction needs readable text, while selection retains the
// separately tested character geometry path.
#[test]
fn inspection_text_preserves_mixed_fonts_columns_and_opening_page_numbers() -> PdfResult<()> {
    let bytes = runtime::run(|| {
        let mut document = shared_pdfium()?.create_new_pdf().unwrap();
        let regular = document.fonts_mut().helvetica();
        let bold = document.fonts_mut().helvetica_bold();
        {
            let mut page = document.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
            let mut x = PdfPoints::new(40.0);
            for (part, font) in [("ISBN: ", regular), ("978-0-8213-3827-8", bold), (" (PDF)", regular)] {
                let object = page.objects_mut().create_text_object(x, PdfPoints::new(720.0), part, font, PdfPoints::new(12.0)).unwrap();
                x = PdfPoints::new(object.bounds().unwrap().right().value + 2.0);
            }
            page.objects_mut().create_text_object(PdfPoints::new(340.0), PdfPoints::new(720.0), "Other column", regular, PdfPoints::new(12.0)).unwrap();
            page.objects_mut().create_text_object(PdfPoints::new(340.0), PdfPoints::new(700.0), "ISBN: 9780192802156 (paperback)", regular, PdfPoints::new(12.0)).unwrap();
        }
        for value in ["Middle page must not be inspected", "Closing page"] {
            let mut page = document.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
            page.objects_mut().create_text_object(PdfPoints::new(40.0), PdfPoints::new(720.0), value, regular, PdfPoints::new(12.0)).unwrap();
        }
        Ok(document.save_to_bytes().unwrap())
    })?;
    let (count, pages) = inspect_page_text_with_count(Cursor::new(bytes), 1, 65536)?;
    assert_eq!(count, 3);
    assert_eq!(pages.iter().map(|(number, _)| *number).collect::<Vec<_>>(), [1]);
    let first = &pages[0].1;
    assert!(first.contains("ISBN: 978-0-8213-3827-8 (PDF)"), "{first}");
    assert!(first.contains("ISBN: 9780192802156 (paperback)"), "{first}");
    assert_eq!(first.matches("978-0-8213-3827-8").count(), 1);
    Ok(())
}

#[test]
fn inspection_text_respects_offset_page_boxes_and_rotations() -> PdfResult<()> {
    for rotation in [PdfPageRenderRotation::None, PdfPageRenderRotation::Degrees90, PdfPageRenderRotation::Degrees180, PdfPageRenderRotation::Degrees270] {
        let bytes = runtime::run(move || {
            let mut document = shared_pdfium()?.create_new_pdf().unwrap();
            let font = document.fonts_mut().helvetica();
            {
                let mut page = document.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
                page.objects_mut().create_text_object(PdfPoints::new(150.0), PdfPoints::new(500.0), "Visible ISBN: 9780821338278", font, PdfPoints::new(12.0)).unwrap();
                page.objects_mut().create_text_object(PdfPoints::new(100.0), PdfPoints::new(760.0), "Hidden ISBN: 9780192802156", font, PdfPoints::new(12.0)).unwrap();
                page.boundaries_mut().set_media(PdfRect::new(PdfPoints::new(72.0), PdfPoints::new(90.0), PdfPoints::new(792.0), PdfPoints::new(612.0))).unwrap();
                page.boundaries_mut().set_crop(PdfRect::new(PdfPoints::new(100.0), PdfPoints::new(120.0), PdfPoints::new(700.0), PdfPoints::new(550.0))).unwrap();
                page.set_rotation(rotation);
            }
            Ok(document.save_to_bytes().unwrap())
        })?;
        let pages = inspect_page_text(Cursor::new(bytes), 1, 65536)?;
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].1, "Visible ISBN: 9780821338278", "{rotation:?}");
    }
    Ok(())
}

#[test]
fn inspection_text_enforces_character_and_utf8_byte_limits() -> PdfResult<()> {
    let bytes = runtime::run(|| {
        let mut document = shared_pdfium()?.create_new_pdf().unwrap();
        let font = document.fonts_mut().helvetica();
        for value in ["a".repeat(80), "é".repeat(80)] {
            let mut page = document.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
            page.objects_mut().create_text_object(PdfPoints::new(40.0), PdfPoints::new(720.0), value, font, PdfPoints::new(6.0)).unwrap();
        }
        Ok(document.save_to_bytes().unwrap())
    })?;
    assert!(inspect_page_text(Cursor::new(bytes.clone()), 2, 60)?.is_empty());
    let pages = inspect_page_text(Cursor::new(bytes), 2, 120)?;
    assert_eq!(pages.len(), 1, "multi-byte text must satisfy the byte limit too");
    assert_eq!(pages[0].1, "a".repeat(80));
    Ok(())
}
