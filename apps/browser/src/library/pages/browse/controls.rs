use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Anchor, AnyElement, App, Context, Entity, Focusable, IntoElement, Subscription, Window, div, px};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{Icon, IconName};
use ui_components as components;

use super::page::BrowsePage;

use library_model::{BrowseBookSort, BrowseChipSort, LibraryBrowseQuery, LibraryFileTypeFilter, LibraryFormatCount, LibraryLanguageCount};

const LIBRARY_LANGUAGE_OPTIONS: [(&str, &str); 8] = [("eng", "English"), ("swe", "Swedish"), ("nor", "Norwegian"), ("dan", "Danish"), ("deu", "German"), ("fra", "French"), ("spa", "Spanish"), ("ita", "Italian")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseFormatFilter {
    Any,
    Library(LibraryFileTypeFilter),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseOptionsSelection {
    pub(crate) search: String,
    pub(super) formats: Vec<BrowseFormatFilter>,
    pub(super) chip_sort: Option<BrowseChipSort>,
    pub(super) book_sort: Option<BrowseBookSort>,
    pub(super) languages: Vec<&'static str>,
    pub(super) hide_finished: bool,
    pub(super) include_direct_child_books: bool,
}

impl BrowseOptionsSelection {
    pub(crate) fn for_controls(controls: &BrowseControls) -> Self {
        Self {
            search: String::new(),
            formats: Vec::new(),
            chip_sort: controls.collection_sorts.first().copied(),
            book_sort: controls.book_sorts.first().copied(),
            languages: Vec::new(),
            hide_finished: false,
            include_direct_child_books: false,
        }
    }

    pub(crate) fn backend_query(&self, location: library_model::BrowseLocation, include_search: bool) -> LibraryBrowseQuery {
        let file_types = self
            .formats
            .iter()
            .filter_map(|format| match format {
                BrowseFormatFilter::Library(format) => Some(*format),
                BrowseFormatFilter::Any => None,
            })
            .collect::<Vec<LibraryFileTypeFilter>>();
        LibraryBrowseQuery {
            location,
            search: if include_search { self.search.clone() } else { String::new() },
            file_types,
            chip_sort: self.chip_sort.unwrap_or_default(),
            book_sort: self.book_sort.unwrap_or_default(),
            languages: self.languages.iter().map(|language| (*language).to_owned()).collect(),
            hide_finished: self.hide_finished,
            include_direct_child_books: self.include_direct_child_books,
        }
    }

    /// Applies a choice from the sort and filter controls, reporting whether
    /// it changed anything. A choice the controls do not offer is ignored.
    pub(crate) fn apply(&mut self, event: BrowseOptionsEvent, controls: &BrowseControls) -> bool {
        match event {
            BrowseOptionsEvent::SelectChipSort(sort) => controls.collection_sorts.contains(&sort) && std::mem::replace(&mut self.chip_sort, Some(sort)) != Some(sort),
            BrowseOptionsEvent::SelectBookSort(sort) => controls.book_sorts.contains(&sort) && std::mem::replace(&mut self.book_sort, Some(sort)) != Some(sort),
            BrowseOptionsEvent::ToggleFormat(format) => toggle(&mut self.formats, (format != BrowseFormatFilter::Any).then_some(format), &controls.formats),
            BrowseOptionsEvent::ToggleLanguage(language) => toggle(&mut self.languages, language, &controls.languages),
            BrowseOptionsEvent::ToggleHideFinished => {
                self.hide_finished = !self.hide_finished;
                true
            }
            BrowseOptionsEvent::ToggleDirectChildBooks => {
                self.include_direct_child_books = !self.include_direct_child_books;
                true
            }
        }
    }
}

/// Toggles `choice` in `selected`, or clears `selected` for `None`, the "any"
/// row. Reports whether anything changed.
fn toggle<T: Copy + PartialEq>(selected: &mut Vec<T>, choice: Option<T>, offered: &[T]) -> bool {
    let Some(choice) = choice else {
        let changed = !selected.is_empty();
        selected.clear();
        return changed;
    };
    if !offered.contains(&choice) {
        return false;
    }
    match selected.iter().position(|item| *item == choice) {
        Some(index) => {
            selected.remove(index);
        }
        None => selected.push(choice),
    }
    true
}

#[derive(Clone, Copy)]
pub(crate) enum BrowseOptionsEvent {
    SelectChipSort(BrowseChipSort),
    SelectBookSort(BrowseBookSort),
    ToggleFormat(BrowseFormatFilter),
    ToggleLanguage(Option<&'static str>),
    ToggleHideFinished,
    ToggleDirectChildBooks,
}

impl BrowseFormatFilter {
    fn label(self) -> &'static str {
        match self {
            Self::Any => "All",
            Self::Library(format) => format.label(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseControls {
    /// The finished filter is offered only where reading progress exists.
    pub(crate) supports_reading_progress: bool,
    pub(crate) supports_direct_child_books: bool,
    pub(crate) collection_sorts: Vec<BrowseChipSort>,
    pub(crate) book_sorts: Vec<BrowseBookSort>,
    pub(crate) formats: Vec<BrowseFormatFilter>,
    format_counts: Vec<(BrowseFormatFilter, u32)>,
    pub(crate) languages: Vec<&'static str>,
    language_counts: Vec<(&'static str, u32)>,
}

impl BrowseControls {
    pub(crate) fn library() -> Self {
        Self {
            supports_reading_progress: true,
            supports_direct_child_books: false,
            collection_sorts: BrowseChipSort::ALL.to_vec(),
            book_sorts: BrowseBookSort::ALL.to_vec(),
            formats: Vec::new(),
            format_counts: Vec::new(),
            languages: Vec::new(),
            language_counts: Vec::new(),
        }
    }

    pub(crate) fn library_with_facets(language_counts: &[LibraryLanguageCount], format_counts: &[LibraryFormatCount]) -> Self {
        let mut controls = Self::library();
        controls.language_counts = LIBRARY_LANGUAGE_OPTIONS.into_iter().filter_map(|(value, _)| language_counts.iter().find(|count| count.language == value).map(|count| (value, count.book_count))).collect();
        if controls.language_counts.len() > 1 || controls.language_counts.iter().any(|(_, count)| *count == 0) {
            controls.languages = controls.language_counts.iter().map(|(language, _)| *language).collect();
        } else {
            controls.language_counts.clear();
        }
        controls.format_counts = format_counts.iter().map(|count| (BrowseFormatFilter::Library(count.format), count.book_count)).collect();
        if controls.format_counts.len() > 1 || controls.format_counts.iter().any(|(_, count)| *count == 0) {
            controls.formats = std::iter::once(BrowseFormatFilter::Any).chain(controls.format_counts.iter().map(|(format, _)| *format)).collect();
        } else {
            controls.format_counts.clear();
        }
        controls
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
        components::browser_search_control(&self.input)
    }
}

/// Sort and filter choices use the same popup-menu surface as the Add action
/// on wide windows. On a compact window the popup menu widget has no bottom
/// sheet mode, so the trigger instead toggles `browse_options_sheet` below.
/// `menu_anchor` is the menu's corner at the trigger.
pub(super) fn browse_options_control(page: Entity<BrowsePage>, controls: BrowseControls, selection: BrowseOptionsSelection, compact: bool, menu_anchor: Anchor, theme: components::BrowserTheme) -> AnyElement {
    let active = selection.chip_sort != controls.collection_sorts.first().copied()
        || selection.book_sort != controls.book_sorts.first().copied()
        || !selection.formats.is_empty()
        || !selection.languages.is_empty()
        || selection.hide_finished
        || selection.include_direct_child_books;
    let trigger = components::outlined_icon_button("browse-options", "Sort & filter", IconName::Settings2, theme) // Deliberately not the selected treatment. This is a dropdown trigger, and
        // `active` means filters are applied — a status on the control, not a state
        // the control is in. An accent edge marks it without claiming to be the
        // chosen thing in the toolbar.
        .when(active, |button| button.border_color(theme.text_accent).text_color(theme.text_accent));

    if compact {
        return trigger.on_click(move |_, _, cx| page.update(cx, |page, cx| page.toggle_browse_options_sheet(cx))).into_any_element();
    }

    trigger
        .dropdown_menu_with_anchor(menu_anchor, move |menu, _, _| {
            let mut menu = menu.scrollable(true);
            for (index, section) in controls.options(&selection).into_iter().enumerate() {
                if index > 0 || section.title != Some("Sort books") {
                    menu = menu.separator();
                }
                if let Some(title) = section.title {
                    menu = menu.item(components::menu_section(title));
                }
                for option in section.options {
                    let page = page.clone();
                    menu = menu.item(
                        components::menu_item(option.label, move |_, _, cx| {
                            page.update(cx, |page, cx| page.select_option(option.event, cx));
                        })
                        .checked(option.checked),
                    );
                }
            }
            menu
        })
        .into_any_element()
}

/// The compact counterpart to `browse_options_control`'s dropdown: the same
/// sections, events, and read of `controls`/`selection`, laid out as full-width
/// rows in a bottom sheet instead of a small anchored popup, since the popup
/// menu widget used above has no bottom-sheet mode of its own.
pub(super) fn browse_options_sheet(page: Entity<BrowsePage>, controls: BrowseControls, selection: BrowseOptionsSelection, theme: components::BrowserTheme) -> AnyElement {
    let mut rows = div().flex().flex_col();

    for (index, section) in controls.options(&selection).into_iter().enumerate() {
        if index > 0 || section.title != Some("Sort books") {
            rows = rows.child(components::bottom_sheet_divider(theme));
        }
        if let Some(title) = section.title {
            rows = rows.child(components::bottom_sheet_section(title, theme));
        }
        for option in section.options {
            let page = page.clone();
            rows = rows.child(components::bottom_sheet_row(option.id, option.label, option.checked, theme).on_click(move |_, _, cx| {
                cx.stop_propagation();
                page.update(cx, |page, cx| page.select_option(option.event, cx));
            }));
        }
    }

    let header_dismiss_target = page.clone();
    let header = components::bottom_sheet_header("Sort & filter", theme).child(
        div()
            .id("browse-options-sheet-close")
            .cursor_pointer()
            .text_color(theme.text_muted)
            .child(Icon::new(IconName::Close).size(px(16.0)))
            .on_click(move |_, _, cx| header_dismiss_target.update(cx, |page, cx| page.close_browse_options_sheet(cx))),
    );

    let sheet = components::bottom_sheet_surface(theme).id("browse-options-sheet").child(header).child(div().id("browse-options-sheet-rows").flex_1().min_h_0().overflow_y_scroll().child(rows));

    components::bottom_sheet_overlay().child(components::modal_scrim("browse-options-sheet-scrim", theme).on_click(move |_, _, cx| page.update(cx, |page, cx| page.close_browse_options_sheet(cx)))).child(sheet).into_any_element()
}

/// Choices are data shared by desktop menus and mobile sheets, not operations.
struct OptionRow {
    id: String,
    label: String,
    checked: bool,
    event: BrowseOptionsEvent,
}
struct OptionSection {
    title: Option<&'static str>,
    options: Vec<OptionRow>,
}

impl BrowseControls {
    fn options(&self, selection: &BrowseOptionsSelection) -> Vec<OptionSection> {
        use BrowseOptionsEvent::*;
        let formats = if self.formats.is_empty() && !selection.formats.is_empty() { vec![BrowseFormatFilter::Any] } else { self.formats.clone() };
        let languages =
            (!self.languages.is_empty() || !selection.languages.is_empty()).then(|| std::iter::once(ToggleLanguage(None)).chain(self.languages.iter().map(|&language| ToggleLanguage(Some(language)))).collect()).unwrap_or_default();
        [
            (Some("Sort books"), self.book_sorts.iter().copied().map(SelectBookSort).collect::<Vec<_>>()),
            (Some("Sort collections"), self.collection_sorts.iter().copied().map(SelectChipSort).collect()),
            (Some("Format"), formats.into_iter().map(ToggleFormat).collect()),
            (Some("Language"), languages),
            (None, self.supports_reading_progress.then_some(ToggleHideFinished).into_iter().collect()),
            (None, self.supports_direct_child_books.then_some(ToggleDirectChildBooks).into_iter().collect()),
        ]
        .into_iter()
        .filter(|(_, events)| !events.is_empty())
        .map(|(title, events)| OptionSection { title, options: events.into_iter().map(|event| self.option(event, selection)).collect() })
        .collect()
    }

    fn option(&self, event: BrowseOptionsEvent, selection: &BrowseOptionsSelection) -> OptionRow {
        use BrowseOptionsEvent::*;
        let (id, label, checked) = match event {
            SelectBookSort(sort) => (format!("sheet-book-sort-{}", sort.label()), sort.label().into(), Some(sort) == selection.book_sort),
            SelectChipSort(sort) => (format!("sheet-chip-sort-{}", sort.label()), sort.label().into(), Some(sort) == selection.chip_sort),
            ToggleFormat(format) => (format!("sheet-format-{}", format.label()), self.format_label(format), if format == BrowseFormatFilter::Any { selection.formats.is_empty() } else { selection.formats.contains(&format) }),
            ToggleLanguage(None) => ("sheet-language-any".into(), "Any".into(), selection.languages.is_empty()),
            ToggleLanguage(Some(language)) => {
                let label = LIBRARY_LANGUAGE_OPTIONS.iter().find(|(value, _)| *value == language).map_or(language, |(_, label)| *label);
                (format!("sheet-language-{language}"), self.language_label(language, label), selection.languages.contains(&language))
            }
            ToggleHideFinished => ("sheet-hide-finished".into(), "Hide finished".into(), selection.hide_finished),
            ToggleDirectChildBooks => ("sheet-direct-child-books".into(), "Include books from direct children".into(), selection.include_direct_child_books),
        };
        OptionRow { id, label, checked, event }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_query_can_omit_search_without_changing_selection_or_filters() {
        let mut selection = BrowseOptionsSelection::for_controls(&BrowseControls::library());
        selection.search = "needle".into();
        selection.formats = vec![BrowseFormatFilter::Library(LibraryFileTypeFilter::Book)];
        selection.languages = vec!["swe"];
        selection.hide_finished = true;
        selection.include_direct_child_books = true;
        let searched = selection.backend_query(library_model::BrowseLocation::Subject(1), true);
        let hierarchy = selection.backend_query(library_model::BrowseLocation::Subject(2), false);
        assert_eq!(searched.search, "needle");
        assert!(hierarchy.search.is_empty());
        assert_eq!(selection.search, "needle");
        assert_eq!(hierarchy.location, library_model::BrowseLocation::Subject(2));
        assert_eq!(hierarchy.file_types, searched.file_types);
        assert_eq!(hierarchy.languages, searched.languages);
        assert_eq!(hierarchy.book_sort, searched.book_sort);
        assert_eq!(hierarchy.chip_sort, searched.chip_sort);
        assert!(hierarchy.hide_finished);
        assert!(hierarchy.include_direct_child_books);
    }

    #[test]
    fn desktop_and_mobile_choices_keep_their_order_labels_and_checked_state() {
        let mut controls = BrowseControls::library();
        controls.formats = vec![BrowseFormatFilter::Any, BrowseFormatFilter::Library(LibraryFileTypeFilter::Book)];
        controls.languages = vec!["eng", "swe"];
        controls.supports_direct_child_books = true;
        let mut selection = BrowseOptionsSelection::for_controls(&controls);
        selection.languages = vec!["swe"];
        selection.formats = vec![BrowseFormatFilter::Library(LibraryFileTypeFilter::Book)];
        let sections = controls.options(&selection);
        assert_eq!(sections.iter().map(|section| section.title).collect::<Vec<_>>(), vec![Some("Sort books"), Some("Sort collections"), Some("Format"), Some("Language"), None, None]);
        assert_eq!(sections[2].options.iter().map(|row| (row.label.as_str(), row.checked)).collect::<Vec<_>>(), vec![("All", false), ("Books", true)]);
        assert_eq!(sections[3].options.iter().map(|row| (row.label.as_str(), row.checked)).collect::<Vec<_>>(), vec![("Any", false), ("English", false), ("Swedish", true)]);
        assert_eq!(sections[3].options[2].id, "sheet-language-swe");
    }

    #[test]
    fn filters_can_be_cleared_when_a_refresh_removes_their_facets() {
        let controls = BrowseControls::library();
        let mut selection = BrowseOptionsSelection::for_controls(&controls);
        selection.formats = vec![BrowseFormatFilter::Library(LibraryFileTypeFilter::Book)];
        selection.languages = vec!["swe"];
        let sections = controls.options(&selection);
        let format = sections.iter().find(|section| section.title == Some("Format")).unwrap();
        let language = sections.iter().find(|section| section.title == Some("Language")).unwrap();
        assert_eq!(format.options.len(), 1);
        assert_eq!(language.options.len(), 1);
        assert!(selection.apply(format.options[0].event, &controls));
        assert!(selection.apply(language.options[0].event, &controls));
        assert!(selection.formats.is_empty());
        assert!(selection.languages.is_empty());
    }
}
