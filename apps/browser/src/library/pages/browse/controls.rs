use gpui::prelude::*;
use gpui::{Anchor, AnyElement, Entity, IntoElement, div, px};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{Icon, IconName};
use ui_components as components;

use super::page::BrowsePage;

use library_model::{BrowseBookSort, BrowseChipSort, BrowseLocation, FeedFacetGroup, LibraryBrowseQuery, LibraryFileTypeFilter, LibraryFormatCount, LibraryLanguageCount};

const LIBRARY_LANGUAGE_OPTIONS: [(&str, &str); 8] = [("eng", "English"), ("swe", "Swedish"), ("nor", "Norwegian"), ("dan", "Danish"), ("deu", "German"), ("fra", "French"), ("spa", "Spanish"), ("ita", "Italian")];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseOptionsSelection {
    pub(crate) search: String,
    pub(super) formats: Vec<LibraryFileTypeFilter>,
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
        LibraryBrowseQuery {
            location,
            search: if include_search { self.search.clone() } else { String::new() },
            file_types: self.formats.clone(),
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
            BrowseOptionsEvent::ToggleFormat(format) => toggle(&mut self.formats, format, &controls.formats),
            BrowseOptionsEvent::ToggleLanguage(language) => toggle(&mut self.languages, language, &controls.languages),
            BrowseOptionsEvent::ToggleHideFinished => {
                self.hide_finished = !self.hide_finished;
                true
            }
            BrowseOptionsEvent::ToggleDirectChildBooks => {
                self.include_direct_child_books = !self.include_direct_child_books;
                true
            }
            // Not a choice the selection keeps: the navigator opens the page.
            BrowseOptionsEvent::OpenFeedPage(_) => false,
        }
    }

    /// Whether any choice differs from what the controls start with. The
    /// search is not one of them: it has its own button.
    fn is_filtered(&self, controls: &BrowseControls) -> bool {
        Self { search: String::new(), ..self.clone() } != Self::for_controls(controls)
    }
}

/// Toggles `choice` in `selected`, or clears `selected` for `None`, the "any"
/// row. Reports whether anything changed.
fn toggle<T: Copy + PartialEq>(selected: &mut Vec<T>, choice: Option<T>, offered: &[Facet<T>]) -> bool {
    let Some(choice) = choice else {
        let changed = !selected.is_empty();
        selected.clear();
        return changed;
    };
    if !offered.iter().any(|facet| facet.value == choice) {
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

/// A filter choice. `None` is the "any" row, which clears the filter.
#[derive(Clone, Copy)]
pub(crate) enum BrowseOptionsEvent {
    SelectChipSort(BrowseChipSort),
    SelectBookSort(BrowseBookSort),
    ToggleFormat(Option<LibraryFileTypeFilter>),
    ToggleLanguage(Option<&'static str>),
    ToggleHideFinished,
    ToggleDirectChildBooks,
    /// One of a feed's own sort or filter choices: the feed page it names.
    OpenFeedPage(BrowseLocation),
}

/// One value a filter offers, and how many books in the listing have it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Facet<T> {
    value: T,
    label: &'static str,
    count: u32,
}

/// A filter is worth offering only where it would narrow the listing: it has
/// more than one value, or a value no book here has.
fn offered<T>(facets: Vec<Facet<T>>) -> Vec<Facet<T>> {
    if facets.len() > 1 || facets.iter().any(|facet| facet.count == 0) { facets } else { Vec::new() }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowseControls {
    pub(crate) supports_direct_child_books: bool,
    pub(crate) collection_sorts: Vec<BrowseChipSort>,
    pub(crate) book_sorts: Vec<BrowseBookSort>,
    formats: Vec<Facet<LibraryFileTypeFilter>>,
    languages: Vec<Facet<&'static str>>,
    /// A feed page's own choices; `None` on a library page.
    feed: Option<Vec<FeedFacetGroup>>,
}

impl BrowseControls {
    pub(crate) fn library() -> Self {
        Self { supports_direct_child_books: false, collection_sorts: BrowseChipSort::ALL.to_vec(), book_sorts: BrowseBookSort::ALL.to_vec(), formats: Vec::new(), languages: Vec::new(), feed: None }
    }

    /// A feed page offers only the feed's own choices: the library's sorts and
    /// filters do not reach a page fetched from the feed.
    pub(crate) fn feed(facets: Vec<FeedFacetGroup>) -> Self {
        Self { supports_direct_child_books: false, collection_sorts: Vec::new(), book_sorts: Vec::new(), formats: Vec::new(), languages: Vec::new(), feed: Some(facets) }
    }

    pub(crate) fn library_with_facets(language_counts: &[LibraryLanguageCount], format_counts: &[LibraryFormatCount]) -> Self {
        let languages = LIBRARY_LANGUAGE_OPTIONS.into_iter().filter_map(|(value, label)| language_counts.iter().find(|count| count.language == value).map(|count| Facet { value, label, count: count.book_count })).collect();
        let formats = format_counts.iter().map(|count| Facet { value: count.format, label: count.format.label(), count: count.book_count }).collect();
        Self { formats: offered(formats), languages: offered(languages), ..Self::library() }
    }
}

/// Sort and filter choices use the same popup-menu surface as the Add action
/// on wide windows. On a compact window the popup menu widget has no bottom
/// sheet mode, so the trigger instead toggles `browse_options_sheet` below.
/// `menu_anchor` is the menu's corner at the trigger.
pub(super) fn browse_options_control(page: Entity<BrowsePage>, controls: BrowseControls, selection: BrowseOptionsSelection, compact: bool, menu_anchor: Anchor, theme: components::BrowserTheme) -> AnyElement {
    let active = selection.is_filtered(&controls);
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
            for section in controls.options(&selection) {
                if section.divided {
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

    for section in controls.options(&selection) {
        if section.divided {
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
    /// Ruled off from what is above it; only a leading "Sort books" is not.
    divided: bool,
    title: Option<String>,
    options: Vec<OptionRow>,
}

impl BrowseControls {
    fn options(&self, selection: &BrowseOptionsSelection) -> Vec<OptionSection> {
        use BrowseOptionsEvent::*;
        if let Some(groups) = &self.feed {
            return groups
                .iter()
                .enumerate()
                .map(|(index, group)| OptionSection {
                    divided: index > 0,
                    title: Some(group.title.clone()),
                    options: group.choices.iter().map(|choice| OptionRow { id: format!("feed-facet-{}", choice.location), label: choice.title.clone(), checked: choice.active, event: OpenFeedPage(choice.location) }).collect(),
                })
                .collect();
        }
        [
            (Some("Sort books"), self.book_sorts.iter().copied().map(SelectBookSort).collect::<Vec<_>>()),
            (Some("Sort collections"), self.collection_sorts.iter().copied().map(SelectChipSort).collect()),
            (Some("Format"), facet_events(&self.formats, &selection.formats, ToggleFormat)),
            (Some("Language"), facet_events(&self.languages, &selection.languages, ToggleLanguage)),
            (None, vec![ToggleHideFinished]),
            (None, self.supports_direct_child_books.then_some(ToggleDirectChildBooks).into_iter().collect()),
        ]
        .into_iter()
        .filter(|(_, events)| !events.is_empty())
        .enumerate()
        .map(|(index, (title, events))| OptionSection { divided: index > 0 || title != Some("Sort books"), title: title.map(str::to_owned), options: events.into_iter().map(|event| self.option(event, selection)).collect() })
        .collect()
    }

    fn option(&self, event: BrowseOptionsEvent, selection: &BrowseOptionsSelection) -> OptionRow {
        use BrowseOptionsEvent::*;
        let (id, label, checked) = match event {
            SelectBookSort(sort) => (format!("sheet-book-sort-{}", sort.label()), sort.label().into(), Some(sort) == selection.book_sort),
            SelectChipSort(sort) => (format!("sheet-chip-sort-{}", sort.label()), sort.label().into(), Some(sort) == selection.chip_sort),
            ToggleFormat(None) => ("sheet-format-All".into(), "All".into(), selection.formats.is_empty()),
            ToggleFormat(Some(format)) => (format!("sheet-format-{}", format.label()), facet_label(&self.formats, format), selection.formats.contains(&format)),
            ToggleLanguage(None) => ("sheet-language-any".into(), "Any".into(), selection.languages.is_empty()),
            ToggleLanguage(Some(language)) => (format!("sheet-language-{language}"), facet_label(&self.languages, language), selection.languages.contains(&language)),
            ToggleHideFinished => ("sheet-hide-finished".into(), "Hide finished".into(), selection.hide_finished),
            ToggleDirectChildBooks => ("sheet-direct-child-books".into(), "Include books from direct children".into(), selection.include_direct_child_books),
            OpenFeedPage(_) => unreachable!("a feed's choices are rows of their own"),
        };
        OptionRow { id, label, checked, event }
    }
}

/// A filter's rows: "any" and then each value offered. A filter no longer
/// offered keeps its "any" row while something is still selected in it, so
/// the selection can be cleared.
fn facet_events<T: Copy>(facets: &[Facet<T>], selected: &[T], event: fn(Option<T>) -> BrowseOptionsEvent) -> Vec<BrowseOptionsEvent> {
    if facets.is_empty() && selected.is_empty() {
        return Vec::new();
    }
    std::iter::once(event(None)).chain(facets.iter().map(|facet| event(Some(facet.value)))).collect()
}

fn facet_label<T: PartialEq>(facets: &[Facet<T>], value: T) -> String {
    facets.iter().find(|facet| facet.value == value).map_or_else(String::new, |facet| format!("{} ({})", facet.label, facet.count))
}
