use super::*;
use crate::navigation::LibrarySession;
use app_preferences::CoverText;
use book_model::{AnnotationAnchor, AnnotationStyle};
use gpui::{AppContext, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, size};
use gpui_component::{Root, WindowExt};
use library_model::{BookCardRow, BookSource, LibraryFileTypeFilter, NotebookRequest};
use std::cell::{Cell, RefCell};
use sync_common::BookId;

struct NotebookFixture(Entity<NotebookRow>);

impl Render for NotebookFixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone()).children(Root::render_dialog_layer(window, cx))
    }
}

#[gpui::test]
async fn notebook_uses_book_card_geometry_and_opens_overlay_immediately(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, session, original, _app_client) = LibrarySession::test_context(cx).await;
    let opened = Rc::new(RefCell::new(Vec::new()));
    let recorded = opened.clone();
    let library = cx.update(|cx| {
        cx.set_global(crate::stores::GeneratedCoversOnly(true));
        cx.set_global(crate::stores::CoverTextSetting(CoverText::Always));
        LibraryContext::new(
            original.management().clone(),
            Rc::new(original.backend().clone()),
            Rc::new(move |locator, _, target, _, _| recorded.borrow_mut().push((locator.book_id(), target))),
            original.updates(),
            session.downgrade(),
            cx.new(|_| crate::NowPlaying::default()),
        )
    });
    let id = BookId::from_bytes([37; 32]);
    let book = BookCardRow {
        source: BookSource::Library { book_id: id },
        title: "Notebook fixture".into(),
        subtitle: Some("The standard subtitle".into()),
        author: "Writer".into(),
        progress: 0.0,
        downloaded: true,
        download_requested: false,
        format_category: LibraryFileTypeFilter::Book,
        audiobook_duration_ms: None,
        added_at: 0,
        source_directory: None,
    };
    let values = (0..4)
        .map(|index| Annotation {
            id: format!("note-{index}"),
            book_id: id,
            anchor: AnnotationAnchor::epub_publication_cfi(format!("epubcfi(/6/2!/4/2/1:{index})")),
            exact_text: if index == 1 { "Short quote".into() } else { "A long highlighted passage, with several words. ".repeat(8) },
            style: AnnotationStyle::Highlight,
            color: 0xe0bb4b,
            note: if index == 1 { String::new() } else { "A longer note to read in full in the overlay. ".repeat(4) },
            created_at: index,
            modified_at: index,
            toc_ordinal: None,
            progress: None,
            deleted: false,
        })
        .collect();
    let row = cx.new(|cx| NotebookRow::new(NotebookBook { book, page: NotebookAnnotationPage { annotations: values, ..Default::default() } }, Default::default(), library, cx));
    let content = row.clone();
    let (_root, visual) = cx.add_window_view(move |window, cx| {
        let content = cx.new(|_| NotebookFixture(content));
        Root::new(content, window, cx)
    });
    visual.simulate_resize(size(px(1000.0), px(750.0)));
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let long = visual.debug_bounds("notebook-annotation-note-0").unwrap();
    let short = visual.debug_bounds("notebook-annotation-note-1").unwrap();
    let book = visual.debug_bounds("notebook-book-card").unwrap();
    assert!((f32::from(book.size.height - long.size.height)).abs() < 1.0);
    assert!(short.size.width < long.size.width);
    let with_text = book.size.height;
    visual.update(|_, cx| cx.set_global(crate::stores::CoverTextSetting(CoverText::CoverOnly)));
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let no_text = visual.debug_bounds("notebook-book-card").unwrap();
    assert!(no_text.size.height < with_text);
    assert!((f32::from(no_text.size.height - visual.debug_bounds("notebook-annotation-note-0").unwrap().size.height)).abs() < 1.0);
    let click = visual.debug_bounds("notebook-annotation-note-0").unwrap().center();
    visual.simulate_event(MouseDownEvent { position: click, button: MouseButton::Left, click_count: 1, ..Default::default() });
    visual.simulate_event(MouseUpEvent { position: click, button: MouseButton::Left, click_count: 1, ..Default::default() });
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("notebook-full-annotation").is_some());
    let editor = visual.debug_bounds("notebook-annotation-editor").expect("single click must open the shared editor");
    assert!((f32::from(editor.size.width) - 360.0).abs() < 1.0);
    assert!(visual.debug_bounds("reader-annotation-note-row").is_some());
    assert!(opened.borrow().is_empty(), "single click must leave the reader closed");
    visual.update(|window, cx| window.close_dialog(cx));
    visual.run_until_parked();
    visual.simulate_event(MouseDownEvent { position: click, button: MouseButton::Left, click_count: 2, ..Default::default() });
    visual.simulate_event(MouseUpEvent { position: click, button: MouseButton::Left, click_count: 2, ..Default::default() });
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("notebook-full-annotation").is_some());
    assert!(opened.borrow().is_empty(), "reader navigation belongs to the overlay's Open in reader button");
    visual.update(|window, cx| window.close_dialog(cx));
    visual.run_until_parked();
    visual.update(|_, cx| {
        row.update(cx, |row, cx| {
            assert!(row.flow.read(cx).has_next());
            row.flow.update(cx, |flow, cx| {
                flow.next_with_axis(PaginatorAxis::Horizontal, cx);
            });
        })
    });
    visual.run_until_parked();
    assert!(visual.update(|_, cx| row.read(cx).flow.read(cx).has_previous()));
}

#[gpui::test]
async fn notebook_row_replaces_annotation_windows_and_enters_previous_at_the_end(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, session, original, _app_client) = LibrarySession::test_context(cx).await;
    let entry = original.management().create_library_with_asset_storage("Paging fixture".into(), false).await.unwrap();
    let library = cx.update(|cx| {
        let app = original.management().clone();
        let backend = Rc::new(app.library(*entry.library_id()).unwrap());
        let updates = crate::library::LibraryUpdateBridge::create(backend.as_ref().clone(), cx);
        LibraryContext::new(app, backend, Rc::new(|_, _, _, _, _| {}), updates, session.downgrade(), cx.new(|_| crate::NowPlaying::default()))
    });
    let backend = library.backend().clone();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/library-backend/crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    assert!(backend.import_paths(sync_common::ROOT_DIR_ID, vec![fixture], false).await.unwrap().is_empty());
    let book = backend
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
            1,
        )
        .await
        .unwrap()
        .books
        .remove(0);
    for index in 0..40 {
        backend
            .upsert_annotation(Annotation {
                id: format!("window-{index:03}"),
                book_id: book.library_file().unwrap(),
                anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"),
                exact_text: "A passage".into(),
                style: AnnotationStyle::Highlight,
                color: 0xe0bb4b,
                note: String::new(),
                created_at: index,
                modified_at: index,
                toc_ordinal: None,
                progress: None,
                deleted: false,
            })
            .await
            .unwrap();
    }
    let data = backend.notebook(Default::default(), Default::default()).await.unwrap().unwrap().books.remove(0);
    let row = cx.new(|cx| NotebookRow::new(data, Default::default(), library, cx));
    let content = row.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let content = cx.new(|_| NotebookFixture(content));
        Root::new(content, window, cx)
    });
    visual.simulate_resize(size(px(1000.0), px(750.0)));
    visual.run_until_parked();
    // Enter the end of the first batch, then exercise the same turn method
    // used by the arrow. The next batch must replace, rather than accumulate.
    visual.update(|_, cx| row.update(cx, |row, cx| row.flow.update(cx, |flow, cx| flow.show_last_page(cx))));
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    visual.update(|_, cx| row.update(cx, |row, cx| row.turn(true, cx)));
    visual.condition(&row, |row, _| row.request.is_none() && row.offset == 32).await;
    row.read_with(visual, |row, _| {
        assert_eq!(row.annotations.len(), 8);
        assert_eq!(row.annotations[0].0.id, "window-007");
        assert!(!row.has_more);
    });
    let window = row.read_with(visual, |row, _| row.annotation_window().unwrap());
    let refreshed = backend.notebook(Default::default(), NotebookRequest { annotation_offsets: [window].into(), ..Default::default() }).await.unwrap().unwrap().books.remove(0);
    visual.update(|_, cx| {
        row.update(cx, |row, cx| {
            row.replace(refreshed, Default::default(), cx);
            assert_eq!(row.offset, 32);
            assert!(row.request.is_none(), "refresh must install the requested window without a follow-up fetch");
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    visual.update(|_, cx| row.update(cx, |row, cx| row.turn(false, cx)));
    visual.condition(&row, |row, _| row.request.is_none() && row.offset == 0).await;
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    row.read_with(visual, |row, cx| {
        assert_eq!(row.annotations.len(), 32);
        assert!(row.flow.read(cx).has_previous(), "previous batch must open on its last local page");
        assert!(!row.flow.read(cx).has_next());
    });
    let stale = Rc::new(Cell::new(false));
    let received = stale.clone();
    let _subscription = visual.update(|_, cx| cx.subscribe(&row, move |_, _: &Stale, _| received.set(true)));
    backend.delete_annotation("window-039".into(), 100).await.unwrap();
    visual.update(|_, cx| row.update(cx, |row, cx| row.turn(true, cx)));
    visual.condition(&row, |row, _| row.request.is_none()).await;
    visual.run_until_parked();
    assert!(stale.get(), "stale annotation paging must request a parent refresh");
    assert_eq!(row.read_with(visual, |row, _| row.offset), 0, "a rejected page leaves the current window installed");
}
