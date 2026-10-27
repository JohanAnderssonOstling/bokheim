use super::*;
use crate::navigation::LibrarySession;
use gpui::{TestAppContext, VisualTestContext, WindowHandle, size};
use library_model::{DownloadProgress, LibraryFileTypeFilter};
use std::cell::{Cell, RefCell};

struct CardFixture {
    views: [Entity<BookCardElement>; 2],
    first: BookCardInteraction,
    width: Pixels,
}

impl Render for CardFixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().children(self.views.iter().enumerate().map(|(index, view)| {
            let interaction = if index == 0 { self.first } else { BookCardInteraction::default() };
            div().id(index).w(self.width).h(px(300.0)).child(BookCardView::present(view.clone(), interaction, window, cx))
        }))
    }
}

fn draw(window: WindowHandle<CardFixture>, cx: &TestAppContext) {
    VisualTestContext::from_window(window.into(), cx).update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn row() -> BookCardRow {
    BookCardRow {
        source: BookSource::Library { content_hash: BookId::from_bytes([7; 32]) },
        title: "A book title".into(),
        subtitle: Some("A subtitle".into()),
        author: "Writer".into(),
        progress: 0.0,
        downloaded: true,
        download_requested: false,
        format_category: LibraryFileTypeFilter::Book,
        audiobook_duration_ms: None,
        added_at: 0,
        source_directory: None,
    }
}

#[gpui::test]
async fn library_selection_download_only_enables_for_missing_books(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, _session, library) = LibrarySession::test_context(cx).await;
    let available = cx.new(|cx| BookCardView::new(row(), library.clone(), Rc::new(|_, _, _| {}), false, false, cx));
    let missing_hash = BookId::from_bytes([8; 32]);
    let missing = cx.new(|cx| BookCardView::new(BookCardRow { source: BookSource::Library { content_hash: missing_hash }, downloaded: false, ..row() }, library.clone(), Rc::new(|_, _, _| {}), false, false, cx));
    let selected = [available, missing];
    assert!(!cx.update(|cx| BookCardView::download_selection_action(&selected, cx).unwrap().is_disabled()), "a mixed selection must offer Download");
    for state in [DownloadState::Queued, DownloadState::Downloading(DownloadProgress::ZERO), DownloadState::Downloaded] {
        library.updates().update(cx, |updates, cx| updates.set_download_state_for_test(missing_hash, state, cx));
        assert!(cx.update(|cx| BookCardView::download_selection_action(&selected, cx).unwrap().is_disabled()), "queued and complete books must not be queued again");
    }
    library.updates().update(cx, |updates, cx| updates.set_download_state_for_test(missing_hash, DownloadState::NotDownloaded, cx));
    assert!(!cx.update(|cx| BookCardView::download_selection_action(&selected, cx).unwrap().is_disabled()), "a removed or failed download must be available again");
    let catalog = cx.new(|cx| BookCardView::new(BookCardRow { source: BookSource::Catalog { content_hash: BookId::from_bytes([9; 32]) }, ..row() }, library, Rc::new(|_, _, _| {}), false, false, cx));
    assert!(cx.update(|cx| BookCardView::download_selection_action(&[catalog], cx).is_none()), "catalog imports use their own download operation");
}

#[gpui::test]
async fn play_opens_books_directly_and_shows_audiobook_detail_before_playback(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, _session, library) = LibrarySession::test_context(cx).await;
    let window = cx.add_window(|_, _| gpui::Empty);
    let actions = Rc::new(RefCell::new(Vec::new()));
    let opened = actions.clone();
    let hash = row().library_file().unwrap();
    let library_id = *library.backend().id();
    let library = LibraryContext::new(
        Rc::new(library.app().clone()),
        Rc::new(library.backend().clone()),
        Rc::new(move |locator, title, target, _, _| {
            assert_eq!(*locator.library_id(), library_id);
            assert_eq!(locator.content_hash(), hash);
            assert_eq!(title, row().title);
            assert_eq!(target, None);
            opened.borrow_mut().push("open");
        }),
        library.updates(),
        library.session(),
        library.now_playing().clone(),
    );
    for format_category in [LibraryFileTypeFilter::Book, LibraryFileTypeFilter::Audiobook] {
        actions.borrow_mut().clear();
        let detail = actions.clone();
        let book = BookCardRow { format_category, ..row() };
        let card = cx.new(|cx| {
            BookCardView::new(
                book,
                library.clone(),
                Rc::new(move |event, _, _| {
                    let BookCardEvent::OpenDetail(book, _) = event else { panic!("expected book details") };
                    assert_eq!(book.format_category, LibraryFileTypeFilter::Audiobook);
                    detail.borrow_mut().push("detail");
                }),
                false,
                false,
                cx,
            )
        });
        window.update(cx, |_, window, cx| BookCardView::play(&card, window, cx)).unwrap();
        let expected = if format_category == LibraryFileTypeFilter::Audiobook { vec!["detail", "open"] } else { vec!["open"] };
        assert_eq!(*actions.borrow(), expected);
    }
}

#[gpui::test]
async fn cached_placements_reuse_neighbors_and_invalidate_interaction_rows_and_typography(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, _session, library) = LibrarySession::test_context(cx).await;
    let queued = BookCardRow { downloaded: false, download_requested: true, ..row() };
    let card = cx.new(|cx| BookCardView::new(queued, library, Rc::new(|_, _, _| {}), false, false, cx));
    card.update(cx, |card, _| {
        card.cover = CoverState::Missing;
    });
    let views = cx.update(|cx| [BookCardView::presentation(card.clone(), BookCardPresentation::Grid, cx), BookCardView::presentation(card.clone(), BookCardPresentation::Section, cx)]);
    let window = cx.open_window(size(px(500.0), px(400.0)), |_, _| CardFixture { views: views.clone(), first: BookCardInteraction::default(), width: px(180.0) });
    draw(window, cx);
    let counts = |cx: &TestAppContext| cx.update(|cx| views.each_ref().map(|view| view.read(cx).renders));
    let initial = counts(cx);
    assert!(initial.iter().all(|&count| count > 0));
    assert_eq!(window.update(cx, |_, window, cx| window.simulate_next_frame(cx)).unwrap(), 0, "queued downloads must not animate");

    // A redraw of the parent must reuse both card subtrees.
    window.update(cx, |_, _, cx| cx.notify()).unwrap();
    draw(window, cx);
    assert_eq!(counts(cx), initial);

    // An animation/local change in one placement must leave its neighbor cached.
    views[0].update(cx, |_, cx| cx.notify());
    draw(window, cx);
    assert_eq!(counts(cx), [initial[0] + 1, initial[1]]);

    for interaction in [
        BookCardInteraction { selected: true, ..BookCardInteraction::default() },
        BookCardInteraction::default(),
        BookCardInteraction { focused: true, ..BookCardInteraction::default() },
        BookCardInteraction::default(),
        BookCardInteraction { marquee: true, ..BookCardInteraction::default() },
        BookCardInteraction::default(),
    ] {
        let before = counts(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.first = interaction;
                cx.notify();
            })
            .unwrap();
        draw(window, cx);
        assert_eq!(counts(cx), [before[0] + 1, before[1]], "interaction must change the current frame, including toggling back");
    }

    let before = counts(cx);
    card.update(cx, |card, cx| {
        let mut row = card.book.clone();
        row.title = "Updated title".into();
        card.replace_row(row, cx);
    });
    draw(window, cx);
    assert_eq!(counts(cx), before.map(|count| count + 1), "model updates invalidate every placement of that book");

    let before = counts(cx);
    window
        .update(cx, |_, window, cx| {
            window.set_rem_size(px(24.0));
            cx.notify();
        })
        .unwrap();
    draw(window, cx);
    assert_eq!(counts(cx), before.map(|count| count + 1), "font scaling must invalidate fixed-size cards");

    let before = counts(cx);
    window
        .update(cx, |fixture, _, cx| {
            fixture.width = px(160.0);
            cx.notify();
        })
        .unwrap();
    draw(window, cx);
    assert_eq!(counts(cx), before.map(|count| count + 1), "resizing must remeasure the card");
}

#[gpui::test]
async fn progress_percentages_do_not_invalidate_cards_but_phase_changes_do(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, _session, library) = LibrarySession::test_context(cx).await;
    let updates = library.updates();
    let card = cx.new(|cx| BookCardView::new(row(), library, Rc::new(|_, _, _| {}), false, false, cx));
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    let _subscription = cx.update(|cx| cx.observe(&card, move |_, _| observed.set(observed.get() + 1)));
    let hash = row().content_hash().unwrap();
    for state in [DownloadState::Queued, DownloadState::Downloading(DownloadProgress::ZERO), DownloadState::Downloading(DownloadProgress::new(0.5).unwrap()), DownloadState::Downloaded] {
        updates.update(cx, |updates, cx| updates.set_download_state_for_test(hash, state, cx));
    }
    assert_eq!(notifications.get(), 3);
}

/// Measure presentation entities separately from book data, images and the
/// window's frame arena. Run alone with LD_PRELOAD=/usr/lib/libtcmalloc.so.
#[cfg(target_os = "linux")]
#[gpui::test]
#[ignore = "live-heap profiling; requires tcmalloc and PROFILE_CARD_COUNT"]
async fn profile_offscreen_presentation_views(cx: &mut TestAppContext) {
    use std::{
        ffi::{CStr, c_char, c_void},
        mem::size_of,
    };

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    let symbol = unsafe { dlsym(std::ptr::null_mut(), c"MallocExtension_GetNumericProperty".as_ptr()) };
    assert!(!symbol.is_null(), "preload libtcmalloc.so for live allocation counters");
    let property: unsafe extern "C" fn(*const c_char, *mut usize) -> i32 = unsafe { std::mem::transmute(symbol) };
    let allocated = || {
        let mut bytes = 0;
        assert_ne!(unsafe { property(c"generic.current_allocated_bytes".as_ptr(), &mut bytes) }, 0);
        bytes
    };
    let heap_checkpoint = |label: &CStr| {
        if std::env::var_os("HEAPPROFILE").is_some() {
            let symbol = unsafe { dlsym(std::ptr::null_mut(), c"HeapProfilerDump".as_ptr()) };
            assert!(!symbol.is_null());
            let dump: unsafe extern "C" fn(*const c_char) = unsafe { std::mem::transmute(symbol) };
            unsafe { dump(label.as_ptr()) };
        }
    };
    let count: usize = std::env::var("PROFILE_CARD_COUNT").expect("set PROFILE_CARD_COUNT").parse().unwrap();
    assert!(count >= 2);
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, _session, library) = LibrarySession::test_context(cx).await;
    cx.update(|cx| cx.set_global(crate::stores::GeneratedCoversOnly(true)));
    let books: Vec<_> = (0..count)
        .map(|index| {
            let mut hash = [0; 32];
            hash[..8].copy_from_slice(&(index as u64).to_le_bytes());
            cx.new(|cx| {
                BookCardView::new(BookCardRow { source: BookSource::Library { content_hash: BookId::from_bytes(hash) }, downloaded: false, download_requested: true, ..row() }, library.clone(), Rc::new(|_, _, _| {}), false, false, cx)
            })
        })
        .collect();
    // Warm the factory before taking the baseline. The same books stay alive
    // throughout; only their presentation entities are created and released.
    drop(cx.update(|cx| BookCardView::presentation(books[0].clone(), BookCardPresentation::Grid, cx)));
    cx.update(|_| {});
    let baseline = allocated();
    heap_checkpoint(c"book data only");
    let mut views: Vec<_> = cx.update(|cx| books.iter().map(|book| BookCardView::presentation(book.clone(), BookCardPresentation::Grid, cx)).collect());
    let created = allocated();
    heap_checkpoint(c"presentation views before rendering");
    assert!(cx.update(|cx| views.iter().all(|view| view.read(cx).renders == 0)));
    let visible = [views[0].clone(), views[1].clone()];
    let window = cx.open_window(size(px(500.0), px(400.0)), |_, _| CardFixture { views: visible, first: BookCardInteraction::default(), width: px(180.0) });
    draw(window, cx);
    let rendered = cx.update(|cx| views.iter().filter(|view| view.read(cx).renders > 0).count());
    assert_eq!(rendered, 2);
    let before_release = allocated();
    heap_checkpoint(c"two visible views before releasing offscreen views");
    views.truncate(2);
    cx.update(|_| {});
    let after_release = allocated();
    heap_checkpoint(c"offscreen views released; all book data retained");
    eprintln!(
        "PRESENTATION_PROFILE {}",
        serde_json::json!({
            "loaded_books": books.len(), "presentation_views_created": count,
            "rendered_views": rendered, "offscreen_views_released": count - rendered,
            "baseline_live_bytes": baseline, "with_presentations_live_bytes": created,
            "presentation_creation_delta_bytes": created as i64 - baseline as i64,
            "before_offscreen_release_live_bytes": before_release, "after_offscreen_release_live_bytes": after_release,
            "offscreen_release_delta_bytes": before_release as i64 - after_release as i64,
            "presentation_struct_bytes": size_of::<BookCardElement>(),
            "test_render_counter_bytes_per_view": size_of::<usize>(),
        })
    );
}
