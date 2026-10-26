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

    for selected in [true, false] {
        let before = counts(cx);
        window
            .update(cx, |fixture, _, cx| {
                fixture.first.selected = selected;
                fixture.first.focused = selected;
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
