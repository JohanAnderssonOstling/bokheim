use super::*;
use crate::library::services::{LibraryAnnotationsChanged, LibraryContentsChanged};
use crate::navigation::LibrarySession;
use book_model::{Annotation, AnnotationAnchor, AnnotationStyle};
use gpui::{TestAppContext, size};
use gpui_component::Root;
use std::rc::Rc;
use sync_common::ROOT_DIR_ID;

#[gpui::test]
async fn notebook_pages_share_browse_refresh_and_preserve_view_state(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, session, original) = LibrarySession::test_context(cx).await;
    let entry = original.app().management.create_library_with_asset_storage("Local notebook fixture".into(), false).await.unwrap();
    let library = cx.update(|cx| {
        let app = Rc::new(original.app().clone());
        let backend = Rc::new(app.management.library(*entry.library_id()));
        let updates = crate::library::LibraryUpdateBridge::create(app.clone(), *entry.library_id(), cx);
        LibraryContext::new(app, original.catalog().clone(), backend, Rc::new(|_, _, _, _, _| {}), updates, session.downgrade(), cx.new(|_| crate::NowPlaying::default()))
    });
    let backend = library.backend().clone();
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/library-backend/crates/book-inspection/tests/fixtures");
    assert!(backend.import_paths(ROOT_DIR_ID, ["back-matter-evidence.epub", "ocr-image-priority.epub"].map(|name| fixtures.join(name)).into(), false).await.unwrap().is_empty());
    let epub = include_bytes!("../../../../../../client/library-backend/tests/fixtures/cloud-cover.epub");
    assert_eq!(&epub[epub.len() - 22..epub.len() - 18], b"PK\x05\x06");
    for index in 0..8 {
        // A unique ZIP comment gives each valid fixture a distinct book identity.
        let mut bytes = epub.to_vec();
        let end = bytes.len();
        bytes[end - 2..].copy_from_slice(&1u16.to_le_bytes());
        bytes.push(b'0' + index);
        backend.import_downloaded_book(ROOT_DIR_ID, format!("Flow book {index}.epub"), bytes).await.unwrap();
    }
    let books = backend
        .browse_section_books(
            library_model::LibraryBrowseQuery {
                location: library_model::BrowseLocation::Books,
                search: String::new(),
                file_types: Vec::new(),
                chip_sort: Default::default(),
                book_sort: Default::default(),
                languages: Vec::new(),
                hide_finished: false,
            },
            0,
            16,
        )
        .await
        .unwrap()
        .books;
    assert_eq!(books.len(), 10);
    let mut annotations = Vec::new();
    for (index, book) in books.iter().enumerate() {
        let annotation = Annotation {
            id: format!("notebook-integration-{index}"),
            book_id: book.library_file().unwrap(),
            anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"),
            exact_text: "A searchable passage".into(),
            note: if index < 2 { "Original note".into() } else { "Other note".into() },
            style: AnnotationStyle::Highlight,
            color: 0xe0bb4b,
            created_at: index as u64 + 1,
            modified_at: index as u64 + 1,
            toc_ordinal: None,
            progress: None,
            deleted: false,
        };
        backend.upsert_annotation(annotation.clone()).await.unwrap();
        annotations.push(annotation);
    }
    cx.run_until_parked();
    let page = cx.new(|cx| NotebookPage::new(library, cx));
    cx.condition(&page, |page, _| page.request.is_none()).await;
    let first = page.read_with(cx, |page, _| {
        assert_eq!(page.rows.len(), BOOK_PAGE_SIZE);
        assert!(page.page.as_ref().unwrap().book_total > page.rows.len());
        page.rows.values().cloned().collect::<Vec<_>>()
    });
    let content = page.clone();
    let (_root, cx) = cx.add_window_view(move |window, cx| Root::new(content, window, cx));
    cx.simulate_resize(size(px(1000.0), px(450.0)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    // The mounted page consumes any library invalidation queued during setup.
    cx.condition(&page, |page, _| page.request.is_none()).await;
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let flow = page.read_with(cx, |page, _| page.flow.clone());
    flow.update(cx, |flow, cx| flow.show_last_page(cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.condition(&page, |page, _| page.request.is_none() && page.rows.len() == 10).await;
    page.read_with(cx, |page, _| {
        assert_eq!(page.rows.values().take(BOOK_PAGE_SIZE).cloned().collect::<Vec<_>>().as_slice(), first.as_slice(), "loading the next visible range keeps existing rows");
        assert_eq!(page.page.as_ref().unwrap().book_total, page.rows.len());
    });
    page.update(cx, |page, cx| {
        page.query.search = "passage".into();
        page.load(None, cx);
    });
    cx.condition(&page, |page, _| page.request.is_none()).await;
    assert_eq!(page.read_with(cx, |page, _| page.rows.len()), BOOK_PAGE_SIZE, "a new search must start with the first batch");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(flow.read_with(cx, |flow, _| flow.visible_child_range().unwrap().start), 0, "a new search returns to the first page");
    page.update(cx, |page, cx| {
        page.query.search = "Original note".into();
        page.query.sort = NotebookSort::BookName;
        page.query.formats = vec![LibraryFileTypeFilter::Book];
        page.load(None, cx);
    });
    cx.condition(&page, |page, _| page.request.is_none()).await;
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let (query, rows) = page.read_with(cx, |page, _| (page.query.clone(), page.rows.values().cloned().collect::<Vec<_>>()));
    assert_eq!(rows.len(), 2);
    flow.update(cx, |flow, cx| flow.show_last_page(cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(flow.read_with(cx, |flow, _| flow.visible_child_range().unwrap().start), 1);
    let edited = page.read_with(cx, |page, _| annotations.iter().position(|annotation| page.cached[&CardKey::from(annotation.book_id)].0 == rows[0]).unwrap());
    annotations[edited].note = "Updated in the reader".into();
    annotations[edited].modified_at = 10;
    backend.upsert_annotation(annotations[edited].clone()).await.unwrap();
    let updates = page.read_with(cx, |page, _| page.library.updates());
    updates.update(cx, |_, cx| {
        cx.emit(LibraryAnnotationsChanged);
        cx.emit(LibraryContentsChanged);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.condition(&page, |page, _| page.request.is_none()).await;
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    page.read_with(cx, |page, cx| {
        assert_eq!(page.query, query);
        assert_eq!(page.rows.values().cloned().collect::<Vec<_>>(), vec![rows[1].clone()], "refresh must apply the edit and reuse the remaining book's paginator");
        assert_eq!(flow.read(cx).visible_child_range().unwrap().start, 0, "the same book stays at the top after its predecessor disappears");
        assert!(page.error.is_none());
    });
    cx.simulate_resize(size(px(1000.0), px(160.0)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let viewport = cx.debug_bounds("notebook-row-viewport").unwrap();
    let book = cx.debug_bounds("notebook-book-card").unwrap();
    assert!(book.size.height > viewport.size.height, "the short window must exercise row overflow");
    cx.simulate_event(gpui::ScrollWheelEvent { position: viewport.center(), delta: gpui::ScrollDelta::Lines(gpui::point(0.0, -100.0)), touch_phase: gpui::TouchPhase::Ended, ..Default::default() });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("notebook-book-card").unwrap().bottom() <= viewport.bottom(), "oversized rows must scroll to their bottom instead of clipping content");
}
