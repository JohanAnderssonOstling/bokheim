use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Anchor, App, Context, Entity, Focusable, IntoElement, Subscription, Window};
use gpui_component::IconName;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::DropdownMenu as _;
use ui_components as components;

use library_model::{BrowseBookSort, BrowseChipSort, LibraryFileTypeFilter, LibraryFormatCount, LibraryLanguageCount};

const LIBRARY_LANGUAGE_OPTIONS: [(&str, &str); 8] = [("eng", "English"), ("swe", "Swedish"), ("nor", "Norwegian"), ("dan", "Danish"), ("deu", "German"), ("fra", "French"), ("spa", "Spanish"), ("ita", "Italian")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseBookOrder {
    Library(BrowseBookSort),
}

impl BrowseBookOrder {
    fn label(self) -> &'static str {
        match self {
            Self::Library(sort) => sort.label(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowseFormatFilter {
    Any,
    Library(LibraryFileTypeFilter),
}

#[derive(Clone)]
pub(crate) struct BrowseOptionsSelection {
    chip_sort: Option<BrowseChipSort>,
    book_sort: Option<BrowseBookOrder>,
    formats: Vec<BrowseFormatFilter>,
    languages: Vec<&'static str>,
    hide_finished: bool,
    include_direct_child_books: bool,
}

impl BrowseOptionsSelection {
    pub(crate) fn new(chip_sort: Option<BrowseChipSort>, book_sort: Option<BrowseBookOrder>, formats: Vec<BrowseFormatFilter>, languages: Vec<&'static str>, hide_finished: bool, include_direct_child_books: bool) -> Self {
        Self { chip_sort, book_sort, formats, languages, hide_finished, include_direct_child_books }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BrowseOptionsEvent {
    SelectChipSort(BrowseChipSort),
    SelectBookSort(BrowseBookOrder),
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
    pub(crate) book_sorts: Vec<BrowseBookOrder>,
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
            book_sorts: BrowseBookSort::ALL.into_iter().map(BrowseBookOrder::Library).collect(),
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

/// Sort and filter choices use the same popup-menu surface as the Add action.
pub(crate) fn browse_options_control<P, SelectOption>(page: Entity<P>, controls: BrowseControls, selection: BrowseOptionsSelection, select_option: SelectOption, theme: components::BrowserTheme) -> impl IntoElement
where
    P: 'static,
    SelectOption: Fn(&mut P, BrowseOptionsEvent, &mut Context<P>) + Clone + 'static,
{
    let active = selection.chip_sort != controls.collection_sorts.first().copied()
        || selection.book_sort != controls.book_sorts.first().copied()
        || !selection.formats.is_empty()
        || !selection.languages.is_empty()
        || selection.hide_finished
        || selection.include_direct_child_books;
    let trigger = components::topbar_icon_action_button("browse-options", "Sort & filter", IconName::Settings2, theme) // Deliberately not the selected treatment. This is a dropdown trigger, and
        // `active` means filters are applied — a status on the control, not a state
        // the control is in. An accent edge marks it without claiming to be the
        // chosen thing in the toolbar.
        .when(active, |button| button.border_color(theme.accent).text_color(theme.accent));
    trigger.dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
        let mut menu = menu.scrollable(true);

        if !controls.book_sorts.is_empty() {
            menu = menu.item(components::menu_section("Sort books"));
            for sort in controls.book_sorts.iter().copied() {
                let page = page.clone();
                let select = select_option.clone();
                menu = menu.item(
                    components::menu_item(sort.label(), move |_, _, cx| {
                        page.update(cx, |page, cx| select(page, BrowseOptionsEvent::SelectBookSort(sort), cx));
                    })
                    .checked(Some(sort) == selection.book_sort),
                );
            }
        }
        if !controls.collection_sorts.is_empty() {
            menu = menu.separator().item(components::menu_section("Sort collections"));
            for sort in controls.collection_sorts.iter().copied() {
                let page = page.clone();
                let select = select_option.clone();
                menu = menu.item(
                    components::menu_item(sort.label(), move |_, _, cx| {
                        page.update(cx, |page, cx| select(page, BrowseOptionsEvent::SelectChipSort(sort), cx));
                    })
                    .checked(Some(sort) == selection.chip_sort),
                );
            }
        }
        if !controls.formats.is_empty() || !selection.formats.is_empty() {
            menu = menu.separator().item(components::menu_section("Format"));
            let formats = if controls.formats.is_empty() { vec![BrowseFormatFilter::Any] } else { controls.formats.clone() };
            for format in formats {
                let checked = if format == BrowseFormatFilter::Any { selection.formats.is_empty() } else { selection.formats.contains(&format) };
                let page = page.clone();
                let select = select_option.clone();
                menu = menu.item(
                    components::menu_item(controls.format_label(format), move |_, _, cx| {
                        page.update(cx, |page, cx| select(page, BrowseOptionsEvent::ToggleFormat(format), cx));
                    })
                    .checked(checked),
                );
            }
        }
        if !controls.languages.is_empty() || !selection.languages.is_empty() {
            menu = menu.separator().item(components::menu_section("Language"));
            let page_for_any = page.clone();
            let select_any = select_option.clone();
            menu = menu.item(
                components::menu_item("Any", move |_, _, cx| {
                    page_for_any.update(cx, |page, cx| select_any(page, BrowseOptionsEvent::ToggleLanguage(None), cx));
                })
                .checked(selection.languages.is_empty()),
            );
            for (language, label) in LIBRARY_LANGUAGE_OPTIONS.into_iter().filter(|(language, _)| controls.languages.contains(language)) {
                let checked = selection.languages.contains(&language);
                let page = page.clone();
                let select = select_option.clone();
                menu = menu.item(
                    components::menu_item(controls.language_label(language, label), move |_, _, cx| {
                        page.update(cx, |page, cx| select(page, BrowseOptionsEvent::ToggleLanguage(Some(language)), cx));
                    })
                    .checked(checked),
                );
            }
        }

        if controls.supports_reading_progress {
            let page = page.clone();
            let select = select_option.clone();
            menu = menu.separator().item(
                components::menu_item("Hide finished", move |_, _, cx| {
                    page.update(cx, |page, cx| select(page, BrowseOptionsEvent::ToggleHideFinished, cx));
                })
                .checked(selection.hide_finished),
            );
        }

        if controls.supports_direct_child_books {
            let page = page.clone();
            let select = select_option.clone();
            menu = menu.separator().item(
                components::menu_item("Include books from direct children", move |_, _, cx| {
                    page.update(cx, |page, cx| select(page, BrowseOptionsEvent::ToggleDirectChildBooks, cx));
                })
                .checked(selection.include_direct_child_books),
            );
        }

        menu
    })
}
