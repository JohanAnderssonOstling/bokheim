use super::super::adapter::BrowseRequest;
use super::super::controls::BrowseControls;
use super::*;
use library_model::{BrowseChipGroup, BrowseRow};

const ROOT: &str = "00000000-0000-0000-0000-000000000000";
const PARENT: &str = "00000000-0000-0000-0000-000000000001";
struct EmptyBook;
impl Render for EmptyBook {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
struct GroupAdapter {
    subject_groups: bool,
    controls: BrowseControls,
    children: Rc<Vec<BrowseRow>>,
    groups: Vec<BrowseChipGroup>,
}
impl GroupAdapter {
    fn new() -> Self {
        let children = (10..40).map(|i| BrowseRow { id: format!("00000000-0000-0000-0000-{i:012}"), name: format!("Subject {i}"), path: None, book_count: 3, downloaded_book_count: 3 }).collect::<Vec<_>>();
        let parent = BrowseRow { id: PARENT.into(), name: "Humanities".into(), path: None, book_count: 90, downloaded_book_count: 90 };
        Self { subject_groups: false, controls: BrowseControls::library(), groups: vec![BrowseChipGroup { parent, start: 0, end: children.len() }], children: Rc::new(children) }
    }
}
impl BrowseAdapter for GroupAdapter {
    type Response = ();
    type Book = EmptyBook;
    fn request(&self, _: &BrowseQuery) -> BrowseRequest<()> {
        BrowseRequest::Ready(())
    }
    fn install(&mut self, _: (), _: &mut App) -> Option<Vec<BrowseCrumb>> {
        None
    }
    fn child_icon(&self) -> components::NavigationIcon {
        components::NavigationIcon::Folder
    }
    fn children(&self) -> Rc<Vec<BrowseRow>> {
        self.children.clone()
    }
    fn chip_groups(&self) -> &[BrowseChipGroup] {
        &self.groups
    }
    fn include_group_in_path(&self) -> bool {
        !self.subject_groups
    }
    fn books(&self) -> Rc<Vec<Entity<EmptyBook>>> {
        Rc::new(Vec::new())
    }
    fn controls(&self) -> &BrowseControls {
        &self.controls
    }
    fn prepare_book(_: &Entity<Self::Book>, _: &mut Window, _: &mut App) {}
}

#[gpui::test]
fn grouped_chips_render_repeated_headers_and_keep_navigation_and_ownership(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let page = cx.new(|cx| BrowsePage::new(GroupAdapter::new(), BrowsePageConfig::new(ROOT, "Folders", "grouped-test").with_split(), cx));
    let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(page.clone(), window, cx));
    for width in [390.0, 1200.0] {
        cx.simulate_resize(gpui::size(px(width), px(700.0)));
        page.update(cx, |page, cx| page.paginator.update(cx, |paginator, cx| paginator.restore_cursor(0, 0, cx)));
        cx.run_until_parked();
        let header = cx.debug_bounds("browse-group-00000000-0000-0000-0000-000000000001").expect("group heading");
        let first = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000010").expect("first chip");
        assert!(header.bottom() <= first.top());
        // Header consumes no child index and cannot be selected as a folder.
        page.update(cx, |page, cx| {
            assert_eq!(page.paginator.read(cx).cursor(), Some(0));
            let targets = page.menu_targets(0, cx).unwrap();
            let parent = DirId::parse_str(PARENT).unwrap();
            assert_eq!(targets.folder_parents[&targets.folders[0]], parent);
            assert!(!targets.all_in_folder(DirId::parse_str(ROOT).unwrap()));
            assert!(targets.all_in_folder(parent));
            page.paginator.update(cx, |paginator, cx| paginator.restore_cursor(20, 20, cx));
        });
        cx.run_until_parked();
        let header = cx.debug_bounds("browse-group-00000000-0000-0000-0000-000000000001").expect("continuation repeats the heading");
        let continued = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000030").expect("continuation chip");
        assert!(header.bottom() <= continued.top());
    }
    cx.simulate_resize(gpui::size(px(390.0), px(60.0)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("browse-group-00000000-0000-0000-0000-000000000001").is_none(), "no orphan heading in a viewport too short for a row: {:?}", cx.debug_bounds("browse-group-00000000-0000-0000-0000-000000000001"));
    cx.simulate_resize(gpui::size(px(1200.0), px(700.0)));
    cx.run_until_parked();
    let continued = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000030").unwrap();
    cx.simulate_click(continued.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    page.read_with(cx, |page, _| {
        assert_eq!(page.path.iter().map(|crumb| crumb.location.as_str()).collect::<Vec<_>>(), [PARENT, "00000000-0000-0000-0000-000000000030"]);
    });
}

#[gpui::test]
fn promoted_subject_skips_the_grouping_in_its_breadcrumb(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let page = cx.new(|cx| {
        let mut adapter = GroupAdapter::new();
        adapter.subject_groups = true;
        BrowsePage::new(adapter, BrowsePageConfig::new(ROOT, "Subjects", "subject-group-test"), cx)
    });
    let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(page.clone(), window, cx));
    cx.simulate_resize(gpui::size(px(390.0), px(700.0)));
    cx.run_until_parked();
    let chip = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000010").unwrap();
    cx.simulate_click(chip.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    page.read_with(cx, |page, _| {
        assert_eq!(page.path.iter().map(|crumb| crumb.location.as_str()).collect::<Vec<_>>(), ["00000000-0000-0000-0000-000000000010"]);
        assert_eq!(page.location, "00000000-0000-0000-0000-000000000010");
    });
}

#[gpui::test]
fn chips_use_intrinsic_widths_and_wrap_at_the_available_edge(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let mut adapter = GroupAdapter::new();
    let children = Rc::make_mut(&mut adapter.children);
    children[0].name = "Art".into();
    children[1].name = "History and philosophy of the humanities".into();
    let page = cx.new(|cx| BrowsePage::new(adapter, BrowsePageConfig::new(ROOT, "Folders", "intrinsic-test"), cx));
    let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(page.clone(), window, cx));
    for width in [390.0, 1200.0] {
        cx.simulate_resize(gpui::size(px(width), px(700.0)));
        cx.run_until_parked();
        let short = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000010").unwrap();
        let long = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000011").unwrap();
        assert!(long.size.width > short.size.width + px(50.0));
        assert!(short.right() <= px(width));
        assert!(long.right() <= px(width));
        if width == 390.0 {
            assert!(long.top() > short.top());
        } else {
            assert_eq!(long.top(), short.top());
        }
    }
    let before = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000010").unwrap();
    cx.update(|window, cx| {
        gpui_component::Theme::global_mut(cx).font_size = px(24.0);
        window.refresh();
    });
    cx.run_until_parked();
    let after = cx.debug_bounds("browse-chip-00000000-0000-0000-0000-000000000010").unwrap();
    assert!(after.size.width > before.size.width);
}
