use super::super::{
    adapter::{BrowseCrumb, BrowseQuery, BrowseRequest},
    controls::BrowseControls,
    page::BrowsePageConfig,
};
use super::*;
use std::cell::RefCell;

struct EmptyBook;
impl Render for EmptyBook {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct TestAdapter {
    queries: Rc<RefCell<Vec<String>>>,
    controls: BrowseControls,
}

struct DownloadAdapter {
    queries: Rc<RefCell<Vec<String>>>,
    downloads: Rc<RefCell<Vec<String>>>,
    complete: Rc<std::cell::Cell<bool>>,
    controls: BrowseControls,
}

impl BrowseAdapter for DownloadAdapter {
    type Response = ();
    type Book = EmptyBook;
    fn request(&self, query: &BrowseQuery) -> BrowseRequest<()> {
        self.queries.borrow_mut().push(query.location.clone());
        BrowseRequest::Ready(())
    }
    fn install(&mut self, _: (), _: &mut App) -> Option<Vec<BrowseCrumb>> {
        None
    }
    fn child_icon(&self) -> components::NavigationIcon {
        components::NavigationIcon::Folder
    }
    fn children(&self) -> Rc<Vec<library_model::BrowseRow>> {
        Rc::new(
            [("partial", 3, if self.complete.get() { 3 } else { 1 }), ("complete", 2, 2), ("empty", 0, 0)]
                .into_iter()
                .map(|(id, book_count, downloaded_book_count)| library_model::BrowseRow { id: id.into(), name: id.into(), path: None, book_count, downloaded_book_count })
                .collect(),
        )
    }
    fn can_download_child(child: &library_model::BrowseRow) -> bool {
        child.downloaded_book_count < child.book_count
    }
    fn download_child(&self, query: &BrowseQuery) -> Option<super::super::adapter::BrowseFuture<Result<(), String>>> {
        self.downloads.borrow_mut().push(query.location.clone());
        Some(Box::pin(async { Ok(()) }))
    }
    fn books(&self) -> Rc<Vec<Entity<EmptyBook>>> {
        Rc::new(Vec::new())
    }
    fn controls(&self) -> &BrowseControls {
        &self.controls
    }
    fn prepare_book(_: &Entity<EmptyBook>, _: &mut Window, _: &mut App) {}
}

#[gpui::test]
fn chip_download_does_not_open_the_folder_and_disappears_when_complete(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let queries = Rc::new(RefCell::new(Vec::new()));
    let downloads = Rc::new(RefCell::new(Vec::new()));
    let complete = Rc::new(std::cell::Cell::new(false));
    let page = cx.new(|cx| {
        BrowsePage::new(DownloadAdapter { queries: queries.clone(), downloads: downloads.clone(), complete: complete.clone(), controls: BrowseControls::library() }, BrowsePageConfig::new("root", "Root", "chip-download-test"), cx)
    });
    let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(page.clone(), window, cx));
    cx.simulate_resize(gpui::size(gpui::px(390.), gpui::px(844.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("chip-download-complete").is_none());
    assert!(cx.debug_bounds("chip-download-empty").is_none());
    let download = cx.debug_bounds("chip-download-partial").expect("download beside total count");
    cx.simulate_click(download.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(&*downloads.borrow(), &["partial"]);
    assert!(queries.borrow().iter().all(|query| query == "root"), "download must not open the chip");
    complete.set(true);
    page.update(cx, |page, cx| page.open_split_path(Vec::new(), cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("chip-download-partial").is_none());
}

#[cfg(all(feature = "mobile", not(feature = "kobo")))]
#[gpui::test]
fn mobile_header_back_navigates_to_the_parent(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let queries = Rc::new(RefCell::new(Vec::new()));
    let page = cx.new(|cx| BrowsePage::new(TestAdapter { queries: queries.clone(), controls: BrowseControls::library() }, BrowsePageConfig::new("root", "Root", "mobile-navigation-test"), cx));
    let (_, cx) = cx.add_window_view(|window, cx| {
        cx.focus_view(&page, window);
        gpui_component::Root::new(page.clone(), window, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(390.), gpui::px(844.)));
    cx.run_until_parked();
    let chip = cx.debug_bounds("browse-chip-child").expect("child folder");
    cx.simulate_click(chip.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(queries.borrow().last().unwrap(), "child");
    let back = cx.debug_bounds("mobile-navigation-back").expect("shared Back button");
    cx.simulate_click(back.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(queries.borrow().last().unwrap(), "root");
}
impl BrowseAdapter for TestAdapter {
    type Response = ();
    type Book = EmptyBook;
    fn request(&self, query: &BrowseQuery) -> BrowseRequest<()> {
        self.queries.borrow_mut().push(query.location.clone());
        BrowseRequest::Ready(())
    }
    fn install(&mut self, _: (), _: &mut App) -> Option<Vec<BrowseCrumb>> {
        None
    }
    fn child_icon(&self) -> components::NavigationIcon {
        components::NavigationIcon::Folder
    }
    fn children(&self) -> Rc<Vec<library_model::BrowseRow>> {
        Rc::new(if self.queries.borrow().last().is_some_and(|location| location == "root") {
            vec![library_model::BrowseRow { id: "child".into(), name: "Child".into(), path: None, book_count: 0, downloaded_book_count: 0 }]
        } else {
            Vec::new()
        })
    }
    fn books(&self) -> Rc<Vec<Entity<EmptyBook>>> {
        Rc::new(Vec::new())
    }
    fn controls(&self) -> &BrowseControls {
        &self.controls
    }
    fn prepare_book(_: &Entity<EmptyBook>, _: &mut Window, _: &mut App) {}
}

#[gpui::test]
fn opening_a_leaf_keeps_the_folder_breadcrumb(cx: &mut gpui::TestAppContext) {
    let queries = Rc::new(RefCell::new(Vec::new()));
    let page = cx.new(|cx| BrowsePage::new(TestAdapter { queries: queries.clone(), controls: BrowseControls::library() }, BrowsePageConfig::new("root", "Root", "leaf-breadcrumb-test"), cx));
    page.update(cx, |page, cx| {
        page.open_split_path(vec![BrowseCrumb { location: "folder".into(), label: "Folder".into() }], cx);
        page.open_leaf(BrowseCrumb { location: "book:abc".into(), label: "A Book".into() }, cx);
    });

    assert_eq!(queries.borrow().last().map(String::as_str), Some("book:abc"));
    page.read_with(cx, |page, _| {
        assert_eq!(page.breadcrumb_labels(), ["Folder"]);
    });
}

#[cfg(not(feature = "mobile"))]
#[gpui::test]
fn chip_modifiers_and_toolbar_open_adjacent_panes(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let plane = cx.new(|cx| {
        BrowseSplitPlane::from_factory(
            Rc::new(|cx| BrowsePage::new(TestAdapter { queries: Rc::new(RefCell::new(Vec::new())), controls: BrowseControls::library() }, BrowsePageConfig::new("root", "Root", "gesture-test").with_split(), cx)),
            cx,
        )
    });
    let original = plane.read_with(cx, |plane, _| plane.panes[0].page.clone());
    let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(plane.clone(), window, cx));
    let control = gpui::Modifiers { control: true, ..Default::default() };
    cx.simulate_resize(gpui::size(gpui::px(320.), gpui::px(640.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("split-browse-button").is_none());
    let chip = cx.debug_bounds("browse-chip-child").unwrap();
    cx.simulate_click(chip.center(), control);
    plane.read_with(cx, |plane, _| assert_eq!(plane.panes.len(), 1));
    original.update(cx, |page, cx| {
        assert_eq!(page.adapter_mut().queries.borrow().last().unwrap(), "child");
        page.open_split_path(Vec::new(), cx);
    });
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    let chip = cx.debug_bounds("browse-chip-child").unwrap();
    cx.simulate_click(chip.center(), control);
    cx.run_until_parked();
    plane.read_with(cx, |plane, _| {
        assert_eq!(plane.panes.len(), 2);
        assert_eq!(plane.panes[1].page, original);
    });
    let chip = cx.debug_bounds("browse-chip-child").unwrap();
    cx.simulate_mouse_down(chip.center(), gpui::MouseButton::Right, control);
    cx.simulate_mouse_up(chip.center(), gpui::MouseButton::Right, control);
    cx.run_until_parked();
    plane.read_with(cx, |plane, _| {
        assert_eq!(plane.panes.len(), 3);
        assert_eq!(plane.panes[1].page, original);
    });
    let button = cx.debug_bounds("split-browse-button").unwrap();
    cx.simulate_click(button.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    plane.read_with(cx, |plane, _| assert_eq!(plane.panes.len(), 4));
}

#[gpui::test]
fn adjacent_splits_keep_queries_and_close_subscriptions_independent(cx: &mut gpui::TestAppContext) {
    let queries = Rc::new(RefCell::new(Vec::<Rc<RefCell<Vec<String>>>>::new()));
    let records = queries.clone();
    let plane = cx.new(|cx| {
        BrowseSplitPlane::from_factory(
            Rc::new(move |cx| {
                let query_log = Rc::new(RefCell::new(Vec::new()));
                records.borrow_mut().push(query_log.clone());
                BrowsePage::new(TestAdapter { queries: query_log, controls: BrowseControls::library() }, BrowsePageConfig::new("root", "Root", "test").with_split(), cx)
            }),
            cx,
        )
    });
    let original = plane.read_with(cx, |plane, _| plane.panes[0].page.clone());
    let split = |page: &Entity<BrowsePage<TestAdapter>>, side, location: &str, cx: &mut gpui::TestAppContext| {
        page.update(cx, |_, cx| cx.emit(BrowseSplitRequest { side, path: vec![BrowseCrumb { location: location.into(), label: location.into() }] }));
        cx.run_until_parked();
    };
    split(&original, BrowseSplitSide::Left, "left", cx);
    split(&original, BrowseSplitSide::Right, "right", cx);
    let left = plane.read_with(cx, |plane, _| {
        assert_eq!(plane.panes.len(), 3);
        assert_eq!(plane.panes[1].page, original);
        plane.panes[0].page.clone()
    });
    assert_eq!(*queries.borrow()[0].borrow(), vec!["root"]);
    assert_eq!(queries.borrow()[1].borrow().last().unwrap(), "left");
    assert_eq!(queries.borrow()[2].borrow().last().unwrap(), "right");
    left.update(cx, |_, cx| cx.emit(BrowseSplitClose));
    cx.run_until_parked();
    split(&original, BrowseSplitSide::Left, "replacement", cx);
    plane.read_with(cx, |plane, _| {
        assert_eq!(plane.panes.len(), 3);
        assert_eq!(plane.panes[1].page, original);
    });
    original.update(cx, |_, cx| cx.emit(BrowseSplitClose));
    cx.run_until_parked();
    plane.read_with(cx, |plane, _| assert_eq!(plane.panes.len(), 2));
}

#[cfg(not(feature = "mobile"))]
#[gpui::test]
fn three_panes_share_width_and_compact_mode_hides_the_split(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let plane = cx.new(|cx| {
        BrowseSplitPlane::from_factory(
            Rc::new(|cx| BrowsePage::new(TestAdapter { queries: Rc::new(RefCell::new(Vec::new())), controls: BrowseControls::library() }, BrowsePageConfig::new("root", "Root", "visual-test").with_split(), cx)),
            cx,
        )
    });
    let original = plane.read_with(cx, |plane, _| plane.panes[0].page.clone());
    for _ in 0..2 {
        original.update(cx, |_, cx| cx.emit(BrowseSplitRequest { side: BrowseSplitSide::Right, path: Vec::new() }));
        cx.run_until_parked();
    }
    let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(plane.clone(), window, cx));
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    let bounds = ["browse-pane-0", "browse-pane-1", "browse-pane-2"].map(|selector| cx.debug_bounds(selector).expect("pane must be rendered"));
    for pane in bounds {
        assert!((pane.size.width - gpui::px(400.)).abs() <= gpui::px(1.));
    }
    cx.simulate_click(bounds[1].center(), gpui::Modifiers::none());
    cx.update(|window, cx| {
        assert!(plane.read(cx).panes[1].page.read(cx).focus_handle(cx).contains_focused(window, cx));
    });
    cx.simulate_resize(gpui::size(gpui::px(320.), gpui::px(640.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("browse-pane-1").is_none());
    cx.update(|window, _| assert!(!crate::split_panes_available(window)));
    cx.update(|window, cx| {
        assert!(plane.read(cx).panes[0].page.read(cx).focus_handle(cx).contains_focused(window, cx));
    });
}
