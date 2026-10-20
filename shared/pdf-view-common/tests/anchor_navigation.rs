use pdf_view_common::{FitWidthAnchor, FitWidthAnchorPager, FitWidthPage, FitWidthViewport, NormalizedRect};

fn pages() -> Vec<(FitWidthPage, Vec<(f32, f32)>)> {
    [2400.0, 800.0, 3200.0, 400.0, 2400.0].into_iter().enumerate().map(|(index, height)| (FitWidthPage::new(index, 600.0, height).unwrap(), vec![(0.32, 0.35), (0.42, 0.45), (0.65, 0.68), (0.75, 0.78)])).collect()
}

#[test]
fn reversing_page_turns_restores_every_visited_viewport() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    for columns in 1..=3 {
        let mut pager = FitWidthAnchorPager::new(pages(), viewport, columns, FitWidthAnchor::new(0, 0.1)).unwrap();
        let mut visited = vec![pager.window().clone()];
        for _ in 0..5 {
            if !pager.forward().unwrap() {
                break;
            }
            visited.push(pager.window().clone());
        }
        assert!(visited.len() > 2);
        for expected in visited.iter().rev().skip(1) {
            assert!(pager.backward().unwrap());
            assert_eq!(pager.window(), expected, "backward must restore the complete viewport for {columns} columns");
        }
        for expected in visited.iter().skip(1) {
            assert!(pager.forward().unwrap());
            assert_eq!(pager.window(), expected, "forward after backward must restore the complete viewport for {columns} columns");
        }
    }
}

#[test]
fn backward_then_forward_restores_an_arbitrary_saved_position() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    for columns in 1..=3 {
        let mut pager = FitWidthAnchorPager::new(pages(), viewport, columns, FitWidthAnchor::new(2, 0.43)).unwrap();
        let saved = pager.window().clone();
        assert!(pager.backward().unwrap());
        assert!(pager.forward().unwrap());
        assert_eq!(pager.window(), &saved, "the reverse calculation must not replace the original saved position");
    }
}

#[test]
fn refreshing_page_geometry_preserves_both_directions_of_history() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    let first = FitWidthAnchor::new(0, 0.1);
    let mut pager = FitWidthAnchorPager::new(pages(), viewport, 2, first).unwrap();
    assert!(pager.forward().unwrap());
    let second = pager.window().start();
    assert!(pager.forward().unwrap());
    let third = pager.window().start();
    assert!(pager.backward().unwrap());
    assert_eq!(pager.window().start(), second);

    let mut updated = pages();
    updated[2].1.push((0.9, 0.95));
    pager.reconfigure(updated, viewport, 2, second).unwrap();
    let refined = FitWidthAnchor::new(second.page_index(), second.page_y() + 0.001);
    pager.restore_start(refined).unwrap();
    assert!(pager.backward().unwrap());
    assert_eq!(pager.window().start(), first);
    assert!(pager.forward().unwrap());
    assert_eq!(pager.window().start(), refined);
    assert!(pager.forward().unwrap());
    assert_eq!(pager.window().start(), third);
}

#[test]
fn changing_layout_or_seeking_starts_a_new_navigation_sequence() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    for new_viewport in [FitWidthViewport::new(600.0, 1000.0).unwrap(), viewport] {
        let mut pager = FitWidthAnchorPager::new(pages(), viewport, 2, FitWidthAnchor::new(0, 0.1)).unwrap();
        assert!(pager.forward().unwrap());
        let start = pager.window().start();
        assert!(pager.forward().unwrap());
        assert!(pager.backward().unwrap());
        let new_columns = if new_viewport == viewport { 1 } else { 2 };
        pager.reconfigure(pages(), new_viewport, new_columns, start).unwrap();
        let mut expected = FitWidthAnchorPager::new(pages(), new_viewport, new_columns, start).unwrap();
        assert!(expected.forward().unwrap());
        assert!(pager.forward().unwrap());
        assert_eq!(pager.window(), expected.window(), "layout changes must discard obsolete forward boundaries");
    }

    let mut pager = FitWidthAnchorPager::new(pages(), viewport, 2, FitWidthAnchor::new(0, 0.1)).unwrap();
    assert!(pager.forward().unwrap());
    assert!(pager.backward().unwrap());
    let destination = FitWidthAnchor::new(2, 0.43);
    pager.seek(destination, false).unwrap();
    let mut expected = FitWidthAnchorPager::new(pages(), viewport, 2, destination).unwrap();
    assert!(expected.forward().unwrap());
    assert!(pager.forward().unwrap());
    assert_eq!(pager.window(), expected.window(), "a direct jump must discard the old forward position");
}

#[test]
fn remembered_seeks_and_boundary_attempts_do_not_lose_positions() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    let first = FitWidthAnchor::new(0, 0.0);
    let last = FitWidthAnchor::new(4, 0.99);
    let mut pager = FitWidthAnchorPager::new(pages(), viewport, 2, first).unwrap();
    assert!(!pager.backward().unwrap());
    pager.seek(last, true).unwrap();
    assert!(!pager.can_forward());
    assert!(!pager.forward().unwrap());
    assert!(pager.can_backward());
    assert!(pager.backward().unwrap());
    assert_eq!(pager.window().start(), first);
    assert!(pager.can_forward());
    assert!(pager.forward().unwrap());
    assert_eq!(pager.window().start(), last);
}

#[test]
fn viewport_work_is_bounded_even_with_a_long_document_or_a_single_tall_page() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    for heights in [vec![2400.0; 10_000], vec![600_000_000.0]] {
        let pages = heights.into_iter().enumerate().map(|(index, height)| (FitWidthPage::new(index, 600.0, height).unwrap(), vec![]));
        let mut pager = FitWidthAnchorPager::new(pages, viewport, 2, FitWidthAnchor::new(0, 0.0)).unwrap();
        for _ in 0..3 {
            assert!(pager.window().plan().len() <= 3, "only the visible columns and one look-ahead column are planned");
            assert!(pager.forward().unwrap());
        }
    }
}

#[test]
fn wheel_pixels_are_independent_of_viewport_column_count_and_page_boundaries() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    for columns in 1..=3 {
        let pages = (0..3).map(|index| (FitWidthPage::new(index, 600.0, 4800.0).unwrap(), vec![]));
        let mut pager = FitWidthAnchorPager::new(pages, viewport, columns, FitWidthAnchor::new(0, 0.0)).unwrap();
        for distance in [799.0, 1.0, 4000.0, 1.0, -1.0, -4000.0, -1.0, -799.0] {
            let before = pager.window().start();
            assert!(pager.scroll_by(distance).unwrap());
            let after = pager.window().start();
            let actual = (after.page_index() as f32 - before.page_index() as f32) * 4800.0 + (after.page_y() - before.page_y()) * 4800.0;
            assert!((actual - distance).abs() < 0.001, "a {distance}px movement travelled {actual}px with {columns} columns");
        }
        assert_eq!(pager.window().start().page_index(), 0);
        assert!(pager.window().start().page_y() * 4800.0 < 0.001);
    }
}

#[test]
fn scrolling_clamps_at_the_end_and_reverses_without_jumping() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    let mut pager = FitWidthAnchorPager::new([(FitWidthPage::new(0, 600.0, 4800.0).unwrap(), vec![])], viewport, 2, FitWidthAnchor::new(0, 0.0)).unwrap();
    assert!(pager.scroll_by(1_000_000.0).unwrap());
    let end = pager.window().start();
    assert!((end.page_y() - 2.0 / 3.0).abs() < 0.00001);
    assert!(!pager.scroll_by(1.0).unwrap());
    assert_eq!(pager.window().start(), end);
    assert!(pager.scroll_by(-1.0).unwrap());
    assert!(((end.page_y() - pager.window().start().page_y()) * 4800.0 - 1.0).abs() < 0.001);
}

#[test]
fn line_boundaries_and_page_turns_share_the_current_scroll_position() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    let mut pager = FitWidthAnchorPager::new(pages(), viewport, 2, FitWidthAnchor::new(0, 0.0)).unwrap();
    let next = pager.adjacent_boundary(true).unwrap();
    pager.seek(next, false).unwrap();
    assert_eq!(pager.adjacent_boundary(false), Some(FitWidthAnchor::new(0, 0.0)));
    pager.scroll_by(17.0).unwrap();
    let scrolled = pager.window().start();
    assert!(pager.forward().unwrap());
    assert!(pager.backward().unwrap());
    assert_eq!(pager.window().start(), scrolled);
}

#[test]
fn protected_bands_leave_short_column_tails_without_blocking_scrolling() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    let page = FitWidthPage::new(0, 600.0, 2400.0).unwrap();
    let bands = vec![(0.32, 0.35), (0.42, 0.45), (0.65, 0.68), (0.75, 0.78)];
    let mut pager = FitWidthAnchorPager::new([(page, bands)], viewport, 2, FitWidthAnchor::new(0, 0.0)).unwrap();
    assert!(pager.scroll_by(16.0).unwrap());
    assert!((pager.window().start().page_y() * 2400.0 - 16.0).abs() < 0.001);
    assert!(pager.scroll_by(10_000.0).unwrap());
    assert!(pager.window().end().is_none(), "the final viewport must reach all remaining content");
}

#[test]
fn crop_analysis_keeps_navigation_history_in_original_pdf_coordinates() {
    let viewport = FitWidthViewport::new(600.0, 800.0).unwrap();
    let page = |index| FitWidthPage::new(index, 600.0, 2400.0).unwrap();
    let original = FitWidthAnchor::new(0, 0.2);
    let next = FitWidthAnchor::new(1, 0.2);
    let mut pager = FitWidthAnchorPager::new([(page(0), vec![]), (page(1), vec![])], viewport, 2, original).unwrap();
    pager.seek(next, true).unwrap();
    let cropped = FitWidthPage::new(0, 480.0, 1920.0).unwrap().with_source_crop(NormalizedRect::new(0.1, 0.1, 0.8, 0.8));
    pager.reconfigure([(cropped, vec![]), (page(1), vec![])], viewport, 2, next).unwrap();
    assert!(pager.backward().unwrap());
    assert_eq!(pager.window().start(), original);
    assert!((pager.window().plan().columns()[0].page_rect().top() - 0.125).abs() < 0.00001);
    assert!(pager.forward().unwrap());
    assert_eq!(pager.window().start(), next);
}
