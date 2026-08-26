use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Anchor, App, Context, Entity, Focusable, IntoElement, MouseButton, Pixels, Subscription, Window, div, px, rems};
use gpui_component::IconName;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::popover::Popover;
use gpui_component::scroll::ScrollableElement;
use ui_components as components;

use app_contract::catalog::{CatalogBookFormat, CatalogSubjectBrowseCapabilities, CatalogSubjectSort};
use app_contract::library::{BrowseBookSort, BrowseChipSort, LibraryFileTypeFilter};

const BROWSE_OPTIONS_POPOVER_GAP: f32 = 4.0;
const BROWSE_OPTIONS_POPOVER_WINDOW_MARGIN: f32 = 8.0;
const BROWSE_OPTIONS_PANEL_MAX_HEIGHT_REM: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct BrowseOptionsGeometry {
    top_offset: f32,
    max_height: f32,
}

fn browse_options_geometry(viewport_height: Pixels, rem_size: Pixels) -> BrowseOptionsGeometry {
    let rem_size = f32::from(rem_size);
    let topbar_height = components::TOPBAR_HEIGHT_REM * rem_size;
    let top_offset = (topbar_height + components::TOPBAR_ACTION_HEIGHT) / 2.0 + BROWSE_OPTIONS_POPOVER_GAP;
    let available_height = (f32::from(viewport_height) - topbar_height - BROWSE_OPTIONS_POPOVER_GAP - BROWSE_OPTIONS_POPOVER_WINDOW_MARGIN).max(1.0);
    let max_height = available_height.min(BROWSE_OPTIONS_PANEL_MAX_HEIGHT_REM * rem_size);
    BrowseOptionsGeometry { top_offset, max_height }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseBookOrder {
    Library(BrowseBookSort),
    Catalog(CatalogSubjectSort),
}

impl BrowseBookOrder {
    fn label(self) -> &'static str {
        match self {
            Self::Library(sort) => sort.label(),
            Self::Catalog(sort) => sort.label(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseFormatFilter {
    Any,
    Library(LibraryFileTypeFilter),
    Catalog(CatalogBookFormat),
}

#[derive(Clone)]
pub(crate) struct BrowseOptionsSelection {
    chip_sort: Option<BrowseChipSort>,
    book_sort: Option<BrowseBookOrder>,
    formats: Vec<BrowseFormatFilter>,
    languages: Vec<&'static str>,
}

impl BrowseOptionsSelection {
    pub(crate) fn new(chip_sort: Option<BrowseChipSort>, book_sort: Option<BrowseBookOrder>, formats: Vec<BrowseFormatFilter>, languages: Vec<&'static str>) -> Self {
        Self { chip_sort, book_sort, formats, languages }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BrowseOptionsEvent {
    SelectChipSort(BrowseChipSort),
    SelectBookSort(BrowseBookOrder),
    ToggleFormat(BrowseFormatFilter),
    ToggleLanguage(Option<&'static str>),
}

impl BrowseFormatFilter {
    fn label(self) -> &'static str {
        match self {
            Self::Any => "All",
            Self::Library(format) => format.label(),
            Self::Catalog(format) => format.label(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseControls {
    pub(crate) collection_sorts: Vec<BrowseChipSort>,
    pub(crate) book_sorts: Vec<BrowseBookOrder>,
    pub(crate) formats: Vec<BrowseFormatFilter>,
    format_counts: Vec<(BrowseFormatFilter, u32)>,
    pub(crate) languages: Vec<&'static str>,
    language_counts: Vec<(&'static str, u32)>,
}

impl BrowseControls {
    pub(crate) fn library() -> Self {
        Self {
            collection_sorts: BrowseChipSort::ALL.to_vec(),
            book_sorts: BrowseBookSort::ALL.into_iter().map(BrowseBookOrder::Library).collect(),
            formats: std::iter::once(BrowseFormatFilter::Any).chain(LibraryFileTypeFilter::ALL.into_iter().filter(|format| *format != LibraryFileTypeFilter::All).map(BrowseFormatFilter::Library)).collect(),
            format_counts: Vec::new(),
            languages: app_contract::preferences::CATALOG_LANGUAGE_OPTIONS.into_iter().map(|(value, _)| value).collect(),
            language_counts: Vec::new(),
        }
    }

    pub(crate) fn catalog() -> Self {
        Self {
            collection_sorts: Vec::new(),
            book_sorts: CatalogSubjectSort::ALL.into_iter().map(BrowseBookOrder::Catalog).collect(),
            formats: std::iter::once(BrowseFormatFilter::Any).chain(CatalogBookFormat::ALL.into_iter().map(BrowseFormatFilter::Catalog)).collect(),
            format_counts: Vec::new(),
            languages: app_contract::preferences::CATALOG_LANGUAGE_OPTIONS.into_iter().map(|(value, _)| value).collect(),
            language_counts: Vec::new(),
        }
    }

    pub(crate) fn catalog_capabilities(capabilities: &CatalogSubjectBrowseCapabilities) -> Self {
        if capabilities.sorts.is_empty() && capabilities.book_formats.is_empty() && capabilities.languages.is_empty() {
            return Self::catalog();
        }
        let formats =
            if capabilities.book_formats.is_empty() { Self::catalog().formats } else { std::iter::once(BrowseFormatFilter::Any).chain(capabilities.book_formats.iter().map(|format| BrowseFormatFilter::Catalog(format.value))).collect() };
        let languages = if capabilities.languages.is_empty() {
            Self::catalog().languages
        } else {
            app_contract::preferences::CATALOG_LANGUAGE_OPTIONS.into_iter().filter_map(|(value, _)| capabilities.languages.iter().any(|language| language.value == value).then_some(value)).collect()
        };
        let format_counts = capabilities.book_formats.iter().map(|format| (BrowseFormatFilter::Catalog(format.value), format.number_of_items)).collect();
        let language_counts =
            capabilities.languages.iter().filter_map(|language| app_contract::preferences::CATALOG_LANGUAGE_OPTIONS.into_iter().find(|(value, _)| *value == language.value).map(|(value, _)| (value, language.number_of_items))).collect();
        let book_sorts = if capabilities.sorts.is_empty() { Self::catalog().book_sorts } else { capabilities.sorts.iter().copied().map(BrowseBookOrder::Catalog).collect() };
        Self { collection_sorts: Vec::new(), book_sorts, formats, format_counts, languages, language_counts }
    }

    fn format_label(&self, format: BrowseFormatFilter) -> String {
        match self.format_counts.iter().find(|(value, _)| *value == format) {
            Some((_, count)) => format!("{} ({count})", format.label()),
            None => format.label().to_owned(),
        }
    }

    fn language_label(&self, language: &'static str, label: &'static str) -> String {
        match self.language_counts.iter().find(|(value, _)| *value == language) {
            Some((_, count)) => format!("{label} ({count})"),
            None => label.to_owned(),
        }
    }
}

/// A query input that reports changes without owning or transforming content.
pub(crate) struct SearchField {
    input: Entity<InputState>,
    suppress_change: Rc<Cell<bool>>,
}

impl SearchField {
    pub(crate) fn new<P: 'static>(placeholder: Option<&'static str>, on_changed: impl Fn(&mut P, &mut Context<P>) + 'static, window: &mut Window, cx: &mut Context<P>) -> (Self, Subscription) {
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            match placeholder {
                Some(placeholder) => input.placeholder(placeholder),
                None => input,
            }
        });
        let suppress_change = Rc::new(Cell::new(false));
        let subscription_suppression = suppress_change.clone();
        let subscription = cx.subscribe(&input, move |page: &mut P, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) && !subscription_suppression.get() {
                on_changed(page, cx);
            }
        });
        (Self { input, suppress_change }, subscription)
    }

    pub(crate) fn query(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    pub(crate) fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus_handle(cx).is_focused(window)
    }

    pub(crate) fn focus<P: 'static>(&self, window: &mut Window, cx: &mut Context<P>) {
        self.input.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(crate) fn clear<P: 'static>(&self, window: &mut Window, cx: &mut Context<P>) {
        self.suppress_change.set(true);
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        self.suppress_change.set(false);
    }

    pub(crate) fn render(&self) -> gpui::Div {
        components::browser_topbar_center().child(components::browser_search_box().child(components::browser_search_input(&self.input)))
    }
}

/// An intrinsic-size dropdown whose choices remain interactive until the user
/// clicks outside it or presses Escape.
pub(crate) fn browse_options_control<P, SelectOption, OpenOptions>(
    page: Entity<P>, controls: BrowseControls, selection: BrowseOptionsSelection, select_option: SelectOption, open_options: OpenOptions, viewport_height: Pixels, rem_size: Pixels, theme: components::BrowserTheme,
) -> impl IntoElement
where
    P: 'static,
    SelectOption: Fn(&mut P, BrowseOptionsEvent, &mut Context<P>) + Clone + 'static,
    OpenOptions: Fn(&mut P, &mut Context<P>) + Clone + 'static,
{
    let geometry = browse_options_geometry(viewport_height, rem_size);
    let active = selection.chip_sort != controls.collection_sorts.first().copied() || selection.book_sort != controls.book_sorts.first().copied() || !selection.formats.is_empty() || !selection.languages.is_empty();
    let trigger = components::topbar_icon_action_button("browse-options", "Sort & filter", IconName::Settings2, theme).when(active, |button| button.bg(theme.accent_soft).border_color(theme.accent));
    let open_page = page.clone();
    Popover::new("browse-options-popover")
        .anchor(Anchor::TopRight)
        .top(px(geometry.top_offset))
        .appearance(false)
        .overlay_closable(true)
        .on_open_change(move |open, _, cx| {
            if *open {
                let open_options = open_options.clone();
                open_page.update(cx, |page, cx| open_options(page, cx));
            }
        })
        .trigger(trigger)
        .content(move |_, _, _| browse_options_panel(page.clone(), controls.clone(), selection.clone(), select_option.clone(), geometry.max_height, theme))
}

fn browse_options_panel<P, SelectOption>(page: Entity<P>, controls: BrowseControls, selection: BrowseOptionsSelection, select_option: SelectOption, max_height: f32, theme: components::BrowserTheme) -> impl IntoElement
where
    P: 'static,
    SelectOption: Fn(&mut P, BrowseOptionsEvent, &mut Context<P>) + Clone + 'static,
{
    let mut content = div().flex().flex_col().gap(px(components::SPACE_MD));

    if !controls.book_sorts.is_empty() || !controls.collection_sorts.is_empty() {
        let mut books = components::browser_settings_button_group(theme);
        for (index, sort) in controls.book_sorts.iter().copied().enumerate() {
            let page = page.clone();
            let select = select_option.clone();
            books = books.child(components::browser_settings_choice_button(format!("browse-book-sort-{index}"), sort.label(), Some(sort) == selection.book_sort, theme).on_click(move |_, _, cx| {
                page.update(cx, |page, cx| select(page, BrowseOptionsEvent::SelectBookSort(sort), cx));
            }));
        }
        let mut collections = components::browser_settings_button_group(theme);
        for (index, sort) in controls.collection_sorts.iter().copied().enumerate() {
            let page = page.clone();
            let select = select_option.clone();
            collections = collections.child(components::browser_settings_choice_button(format!("browse-collection-sort-{index}"), sort.label(), Some(sort) == selection.chip_sort, theme).on_click(move |_, _, cx| {
                page.update(cx, |page, cx| select(page, BrowseOptionsEvent::SelectChipSort(sort), cx));
            }));
        }
        let mut sorting = components::browser_settings_fieldset("Sorting", theme);
        if !controls.book_sorts.is_empty() {
            sorting = sorting.child(components::browser_settings_compact_row("Books", books, theme));
        }
        if !controls.collection_sorts.is_empty() {
            sorting = sorting.child(components::browser_settings_compact_row("Collections", collections, theme));
        }
        content = content.child(sorting);
    }

    if !controls.formats.is_empty() || !controls.languages.is_empty() {
        let mut filters = div().flex().flex_col().gap(px(components::SPACE_SM));
        if !controls.formats.is_empty() {
            let mut formats = components::browser_settings_button_group(theme).flex_wrap().max_w(rems(44.0));
            for (index, format) in controls.formats.iter().copied().enumerate() {
                let checked = if format == BrowseFormatFilter::Any { selection.formats.is_empty() } else { selection.formats.contains(&format) };
                let label = controls.format_label(format);
                let page = page.clone();
                let select = select_option.clone();
                formats = formats.child(components::browser_settings_choice_button(format!("browse-format-{index}"), label, checked, theme).on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| select(page, BrowseOptionsEvent::ToggleFormat(format), cx));
                }));
            }
            filters = filters.child(components::browser_settings_fieldset("Format — match any selected option (OR)", theme).child(formats));
        }
        if !controls.formats.is_empty() && !controls.languages.is_empty() {
            filters = filters.child(div().w_full().text_center().text_color(theme.text_muted).text_size(rems(0.75)).child("AND"));
        }
        if !controls.languages.is_empty() {
            let mut languages = components::browser_settings_button_group(theme).flex_wrap().max_w(rems(44.0));
            let page_for_any = page.clone();
            let select_any = select_option.clone();
            languages = languages.child(components::browser_settings_choice_button("browse-language-any", "Any", selection.languages.is_empty(), theme).on_click(move |_, _, cx| {
                page_for_any.update(cx, |page, cx| select_any(page, BrowseOptionsEvent::ToggleLanguage(None), cx));
            }));
            for (index, (language, label)) in app_contract::preferences::CATALOG_LANGUAGE_OPTIONS.into_iter().filter(|(language, _)| controls.languages.contains(language)).enumerate() {
                let label = controls.language_label(language, label);
                let page = page.clone();
                let select = select_option.clone();
                let checked = selection.languages.contains(&language);
                languages = languages.child(components::browser_settings_choice_button(format!("browse-language-{index}"), label, checked, theme).on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| select(page, BrowseOptionsEvent::ToggleLanguage(Some(language)), cx));
                }));
            }
            filters = filters.child(components::browser_settings_fieldset("Language — match any selected option (OR)", theme).child(languages));
        }
        content = content.child(components::browser_settings_fieldset("Filter", theme).child(filters));
    }

    div()
        .relative()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .min_w_0()
        .min_h_0()
        .occlude()
        .w_auto()
        .h_auto()
        .max_w(rems(44.0))
        .max_h(px(max_height))
        .overflow_y_scrollbar()
        .p(px(components::SPACE_MD))
        .flex()
        .flex_col()
        .gap(px(components::SPACE_MD))
        .rounded(px(components::RADIUS_MD))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow(components::overlay_shadow(theme))
        .child(content)
}

#[cfg(test)]
mod browse_options_geometry_tests {
    use super::*;

    #[test]
    fn options_panel_begins_below_the_topbar() {
        let rem_size = 16.0;
        let geometry = browse_options_geometry(px(800.0), px(rem_size));
        let topbar_height = components::TOPBAR_HEIGHT_REM * rem_size;
        let trigger_top = (topbar_height - components::TOPBAR_ACTION_HEIGHT) / 2.0;

        assert_eq!(trigger_top + geometry.top_offset, topbar_height + BROWSE_OPTIONS_POPOVER_GAP);
    }

    #[test]
    fn options_panel_height_stays_inside_the_viewport() {
        let viewport_height = 320.0;
        let rem_size = 16.0;
        let geometry = browse_options_geometry(px(viewport_height), px(rem_size));
        let topbar_height = components::TOPBAR_HEIGHT_REM * rem_size;

        assert!(geometry.max_height <= viewport_height - topbar_height - BROWSE_OPTIONS_POPOVER_GAP - BROWSE_OPTIONS_POPOVER_WINDOW_MARGIN);
    }
}
