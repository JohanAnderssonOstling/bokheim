//! Shared in-app definition popup for EPUB and PDF selections.
use ui_text::text;

use super::source::{self, Placement as SourcePlacement};

use std::{collections::BTreeMap, future::Future, pin::Pin, rc::Rc};

use content_address::BookId;
use dictionary::{Client as DictionaryClient, Definition as DictionaryDefinition, DictionaryLookup, OperationResult as DictionaryResult};
use dictionary_settings::{Closed as DictionarySettingsClosed, DictionarySettings};
use gpui::prelude::*;
use gpui::{
    Anchor, AnyElement, App, Bounds, ClickEvent, Context, DispatchPhase, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, Task, WeakEntity, Window, canvas, div, point, px, rems, size,
};
use gpui_component::list::{List, ListDelegate, ListEvent, ListItem, ListState};
use gpui_component::popover::{Popover, PopoverState};
use gpui_component::text::TextView;
use gpui_component::{Disableable, FocusTrapElement, IconName, IndexPath, Theme};
use skrift_column_paginator::WORD_SELECTION_DRAG_DISTANCE;
use language_tags::LanguageTag;
use reader_lookups::{WikipediaArticle, WikipediaChoice, WikipediaLookup, LookupError, can_lookup_wikipedia, parse_language_tag};
use ui_components as components;

pub(crate) mod wikipedia;

const LOOKUP_POPUP_PRIORITY: usize = 10;
const LOOKUP_POPUP_WIDTH_REM: f32 = 26.25;
const LOOKUP_POPUP_MAX_HEIGHT: f32 = 420.0;

struct ResultList {
    titles: Vec<String>,
    filtered: Vec<usize>,
    current: usize,
}

impl ListDelegate for ResultList {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.filtered.len()
    }

    fn render_item(&mut self, index: IndexPath, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Option<ListItem> {
        let result = *self.filtered.get(index.row)?;
        Some(
            ListItem::new(index.row)
                .h(rems(components::settings_control_size_rem())).when(components::touch_ui(), |row| row.h(px(48.0)).px(px(components::SPACE_MD)))
                .border_b_1()
                .border_color(components::browser_theme(cx).rule)
                .check_icon(IconName::Check)
                .confirmed(result == self.current)
                .child(div().min_w_0().text_ellipsis().child(self.titles.get(result)?.clone())),
        )
    }

    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}

    fn perform_search(&mut self, query: &str, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Task<()> {
        let query = book_model::normalize_search_text(query);
        self.filtered = self.titles.iter().enumerate().filter_map(|(index, title)| {
            book_model::normalize_search_text(title).contains(&query).then_some(index)
        }).collect();
        cx.notify();
        Task::ready(())
    }
}

/// Popover bodies must paint above the lookup surface, including nested menus.
pub(super) fn lookup_popover(content: impl IntoElement) -> gpui::Deferred {
    gpui::deferred(content).with_priority(LOOKUP_POPUP_PRIORITY + 1)
}

/// Mobile list navigation shares the app's sheet surface and keeps Back beside search.
pub(super) fn mobile_picker<D: ListDelegate>(list: &Entity<ListState<D>>, placeholder: &'static str, window: &mut Window, cx: &mut Context<PopoverState>) -> gpui::Deferred {
    let theme = components::browser_theme(cx);
    let rows = list.read(cx).delegate().items_count(0, cx).max(1);
    let back_target = cx.weak_entity();
    let dismiss_target = cx.entity();
    let right_target = cx.entity();
    let content = List::new(list).search_placeholder(placeholder).search_prefix(move |_, _| {
        let target = back_target.clone();
        components::back_text_button("picker-back", text!(common::back))
            .on_click(move |_, window, cx| { let _ = target.update(cx, |popover, cx| popover.dismiss(window, cx)); }).into_any_element()
    });
    let safe = window.insets().effective();
    let available = window.viewport_size().height - safe.top - safe.bottom;
    let height = (px(48.0) * rows as f32 + window.rem_size() * components::settings_control_size_rem()).min(available * 0.8);
    let panel = components::bottom_sheet_surface(theme).h(height).overflow_hidden()
        .child(div().flex_1().min_h_0().child(content));
    let scrim = components::modal_scrim("lookup-picker-scrim", theme)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| { dismiss_target.update(cx, |popover, cx| popover.dismiss(window, cx)); cx.stop_propagation(); })
        .on_mouse_down(MouseButton::Right, move |_, window, cx| { right_target.update(cx, |popover, cx| popover.dismiss(window, cx)); cx.stop_propagation(); });
    lookup_popover(components::anchor_to_window(components::bottom_sheet_overlay().child(scrim).child(panel), window))
}

/// Desktop pickers replace the visible content within the lookup popup bounds.
pub(super) fn desktop_picker<D: ListDelegate>(list: &Entity<ListState<D>>, placeholder: &'static str, bounds: Option<Bounds<Pixels>>, window: &Window, cx: &mut Context<PopoverState>) -> gpui::Deferred {
    let theme = components::browser_theme(cx);
    let target = cx.weak_entity();
    let content = List::new(list).search_placeholder(placeholder).search_prefix(move |_, _| {
        let target = target.clone();
        components::back_text_button("picker-back", text!(common::back))
            .on_click(move |_, window, cx| { let _ = target.update(cx, |popover, cx| popover.dismiss(window, cx)); }).into_any_element()
    });
    let size = bounds.map_or(gpui::size(window.rem_size() * LOOKUP_POPUP_WIDTH_REM, px(320.0)), |bounds| bounds.size);
    lookup_popover(
        components::modal_form_surface(theme)
            .font_family(Theme::global(cx).font_family.clone())
            .text_size(Theme::global(cx).font_size)
            .w(size.width)
            .h(size.height)
            .max_w(window.viewport_size().width)
            .max_h(window.viewport_size().height)
            .p_0()
            .gap_0()
            .overflow_hidden()
            .occlude()
            .child(div().flex_1().min_h_0().child(content)),
    )
}

pub(crate) struct Closed;
pub(crate) struct SelectElement {
    pub position: Point<Pixels>,
}

#[derive(Default)]
pub(crate) struct WordClick {
    start: Option<(Point<Pixels>, usize)>,
}

impl WordClick {
    pub(crate) fn begin(&mut self, event: &MouseDownEvent) {
        self.start = (event.button == MouseButton::Left && matches!(event.click_count, 2 | 4) && event.modifiers == Default::default()).then_some((event.position, event.click_count));
    }

    pub(crate) fn finish(&mut self, event: &MouseUpEvent) -> bool {
        self.finish_at(event.position, event.click_count, event.button == MouseButton::Left && event.modifiers == Default::default())
    }

    fn finish_click(&mut self, event: &ClickEvent) -> bool {
        self.finish_at(event.position(), event.click_count(), event.mouse_position().is_some() && event.modifiers() == Default::default())
    }

    fn finish_at(&mut self, position: Point<Pixels>, count: usize, valid: bool) -> bool {
        self.start.take().is_some_and(|(start, pressed_count)| valid && count == pressed_count && within_click_distance(start, position))
    }

    pub(crate) fn moved(&mut self, position: Point<Pixels>) {
        if self.start.is_some_and(|(start, _)| !within_click_distance(start, position)) {
            self.start = None;
        }
    }
}

fn within_click_distance(start: Point<Pixels>, end: Point<Pixels>) -> bool {
    let dx = f32::from(end.x - start.x);
    let dy = f32::from(end.y - start.y);
    dx * dx + dy * dy < WORD_SELECTION_DRAG_DISTANCE * WORD_SELECTION_DRAG_DISTANCE
}

fn numbered_meaning(line: &str) -> Option<usize> {
    let mut line = line.trim_start();
    let label_end = line.find(|character: char| !character.is_ascii_alphabetic() && character != '.').unwrap_or(line.len());
    let label = line[..label_end].trim_end_matches('.');
    if [
        "n",
        "v",
        "a",
        "ad",
        "adj",
        "adv",
        "vb",
        "vt",
        "vi",
        "v.t",
        "v.i",
        "v.tr",
        "v.intr",
        "pron",
        "prep",
        "conj",
        "interj",
        "int",
        "num",
        "art",
        "det",
        "aux",
        "noun",
        "verb",
        "adjective",
        "adverb",
        "pronoun",
        "preposition",
        "conjunction",
        "interjection",
        "numeral",
        "article",
        "determiner",
    ]
    .iter()
    .any(|prefix| label.eq_ignore_ascii_case(prefix))
    {
        line = line[label_end..].trim_start_matches(|character: char| character.is_whitespace() || character == ':');
    }
    let parenthesized = line.starts_with('(');
    let number = line.strip_prefix('(').unwrap_or(line);
    let end = number.find(|character: char| !character.is_ascii_digit())?;
    let rest = &number[end..];
    let rest = if parenthesized { rest.strip_prefix(')')? } else { rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))? };
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    number[..end].parse().ok()
}

/// Separate numbered senses, including entries whose first sense is unnumbered, without changing the stored dictionary.
fn definition_text(text: &str) -> String {
    let text = text.trim_start();
    let numbers: Vec<_> = text.lines().filter_map(numbered_meaning).collect();
    if !numbers.contains(&2) {
        return text.to_owned();
    }
    let mut output = String::with_capacity(text.len() + numbers.len());
    let mut previous_blank = true;
    for line in text.lines() {
        if !output.is_empty() {
            output.push('\n');
            if !previous_blank && numbered_meaning(line).is_some() {
                output.push('\n');
            }
        }
        output.push_str(line);
        previous_blank = line.trim().is_empty();
    }
    output
}

type WikipediaRequest = Rc<dyn Fn(String, Vec<LanguageTag>) -> Pin<Box<dyn Future<Output = Option<WikipediaLookup>>>>>;

pub(crate) struct DictionaryPopup {
    translation: Option<Entity<super::translation::TranslationPopup>>,
    show_translation: bool,
    backend: Option<Rc<dyn DictionaryClient>>,
    settings: Option<Entity<DictionarySettings>>,
    query: String,
    result: Option<DictionaryResult<DictionaryLookup>>,
    dictionary_index: usize,
    definitions_scroll: ScrollHandle,
    wheel_delta: f32,
    wheel_debounce: Option<Task<()>>,
    wheel_scrolling_text: bool,
    touchpad: components::TouchpadNavigation,
    downloading: bool,
    selection_only: bool,
    anchor: Option<Bounds<Pixels>>,
    popup_area: Option<(Bounds<Pixels>, Anchor)>,
    popup_height: Option<Pixels>,
    element_click: WordClick,
    focus: FocusHandle,
    wikipedia_request: Option<WikipediaRequest>,
    wikipedia_popup: Option<Entity<Self>>,
    wikipedia: Option<WikipediaArticle>,
    wikipedia_page: wikipedia::ArticlePage,
    wikipedia_equivalents: wikipedia::Equivalents,
    languages: Vec<String>,
    wikipedia_attempted: bool,
    wikipedia_failed: Option<LookupError>,
    wikipedia_hits: Vec<WikipediaChoice>,
    wikipedia_index: usize,
    wikipedia_cache: BTreeMap<String, Result<Option<WikipediaArticle>, LookupError>>,
    wikipedia_loading: bool,
    _dismiss_task: Task<()>,
    _wikipedia_task: Task<()>,
    _task: Task<()>,
}

impl EventEmitter<Closed> for DictionaryPopup {}
impl EventEmitter<SelectElement> for DictionaryPopup {}
impl EventEmitter<WikipediaArticle> for DictionaryPopup {}

impl DictionaryPopup {
    pub(crate) fn new(query: String, languages: Vec<String>, anchor: Option<Bounds<Pixels>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dictionary = dictionary_settings::client(cx);
        let settings_backend = dictionary.clone();
        let lookup = query.clone();
        let lookup_languages = languages.clone();
        let wikipedia_client = super::lookups::client(cx);
        let wikipedia_request: WikipediaRequest = Rc::new(move |query, languages| {
            let client = wikipedia_client.clone();
            Box::pin(async move { client.lookup_wikipedia(query, languages).await })
        });
        let task = cx.spawn(async move |this, cx| {
            let mut changes = dictionary.changes().await.ok();
            loop {
                let result = dictionary.lookup(lookup.clone(), lookup_languages.clone()).await;
                match &result {
                    Err(error) => log::warn!("Dictionary lookup failed: {error}"),
                    Ok(result) => {
                        for error in &result.errors {
                            log::warn!("Dictionary lookup failed: {error}");
                        }
                    }
                }
                let downloading = dictionary.downloads(Vec::new()).await.is_ok_and(|state| state.active.is_some());
                if this
                    .update(cx, |this, cx| {
                        let previous_total = this.dictionary_entries().count();
                        let wikipedia_selected = previous_total > 0 && this.dictionary_index == previous_total;
                        this.result = Some(result);
                        if wikipedia_selected {
                            this.dictionary_index = this.dictionary_entries().count();
                        }
                        this.downloading = downloading;
                        this.refresh_wikipedia(cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                let Some(changes) = changes.as_mut() else {
                    return;
                };
                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
        Self { backend: Some(settings_backend), wikipedia_request: Some(wikipedia_request), _task: task, ..Self::empty(query, languages, anchor, window, cx) }
    }

    fn empty(query: String, languages: Vec<String>, anchor: Option<Bounds<Pixels>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            translation: None,
            show_translation: false,
            backend: None,
            settings: None,
            query,
            result: None,
            dictionary_index: 0,
            definitions_scroll: ScrollHandle::new(),
            wheel_delta: 0.0,
            wheel_debounce: None,
            wheel_scrolling_text: false,
            touchpad: components::TouchpadNavigation::default(),
            downloading: false,
            selection_only: false,
            anchor,
            popup_area: None,
            popup_height: None,
            element_click: WordClick::default(),
            focus,
            wikipedia_request: None,
            wikipedia_popup: None,
            wikipedia: None,
            wikipedia_page: wikipedia::ArticlePage::default(),
            wikipedia_equivalents: wikipedia::Equivalents::default(),
            languages,
            wikipedia_attempted: false,
            wikipedia_failed: None,
            wikipedia_hits: Vec::new(),
            wikipedia_index: 0,
            wikipedia_cache: BTreeMap::new(),
            wikipedia_loading: false,
            _dismiss_task: Task::ready(()),
            _wikipedia_task: Task::ready(()),
            _task: Task::ready(()),
        }
    }

    pub(crate) fn translate(book_id: BookId, query: String, languages: Vec<String>, anchor: Option<Bounds<Pixels>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let translation = cx.new(|cx| super::translation::TranslationPopup::new(book_id, query.clone(), &languages, cx));
        cx.observe(&translation, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&translation, |this, _, _: &super::translation::ResultsChanged, cx| {
            if this.showing_translation() {
                this.popup_height = None;
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&translation, |this, _, event: &super::translation::LanguagesChanged, cx| {
            this.update_translation_languages(event, cx);
        })
        .detach();
        let languages = translation.read(cx).languages();
        let lookups = super::lookups::client(cx);
        let request: WikipediaRequest = Rc::new(move |query, languages| {
            let lookups = lookups.clone();
            Box::pin(async move { lookups.lookup_wikipedia(query, languages).await })
        });
        let mut popup = Self {
            translation: Some(translation),
            show_translation: true,
            result: Some(Ok(DictionaryLookup { dictionaries: Vec::new(), enabled_count: 0, errors: Vec::new() })),
            wikipedia_request: Some(request),
            ..Self::empty(query, Vec::new(), anchor, window, cx)
        };
        popup.update_translation_languages(&languages, cx);
        popup
    }

    fn update_translation_languages(&mut self, languages: &super::translation::LanguagesChanged, cx: &mut Context<Self>) {
        let source = languages.source.as_ref().map(ToString::to_string).into_iter().collect::<Vec<_>>();
        if self.languages != source || self.wikipedia_equivalents.targets != languages.targets {
            self._wikipedia_task = Task::ready(());
            self.wikipedia_page.close();
            self.wikipedia = None;
            self.wikipedia_attempted = false;
            self.wikipedia_failed = None;
            self.wikipedia_loading = false;
            self.wikipedia_hits.clear();
            self.wikipedia_cache.clear();
            self.wikipedia_index = 0;
            self.languages = source;
        }
        self.wikipedia_equivalents = wikipedia::Equivalents::new(languages.targets.clone());
        self.refresh_wikipedia(cx);
        self.refresh_wikipedia_equivalents(cx);
        cx.notify();
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend.clone() else { return };
        self.reset_result_wheel();
        let settings = cx.new(|cx| DictionarySettings::new(backend, self.languages.clone(), cx).closable());
        cx.subscribe_in(&settings, window, |this, _, _: &DictionarySettingsClosed, window, cx| this.close_settings(window, cx)).detach();
        self.settings = Some(settings);
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn render_settings(&self, settings: Entity<DictionarySettings>, window: &Window, cx: &mut Context<Self>) -> gpui::Deferred {
        let bounds = self.picker_bounds().filter(|_| !components::touch_ui());
        let theme = components::browser_theme(cx);
        let insets = window.insets().effective();
        let width = (window.viewport_size().width - insets.left - insets.right - px(32.0)).clamp(px(1.0), window.rem_size() * 40.0);
        let height = (window.viewport_size().height - insets.top - insets.bottom - px(32.0)).clamp(px(1.0), px(640.0));
        let panel = (if components::touch_ui() { components::bottom_sheet_surface(theme) } else { components::modal_form_surface(theme) })
            .font_family(Theme::global(cx).font_family.clone())
            .text_size(Theme::global(cx).font_size)
            .debug_selector(|| "reader-dictionary-settings".into())
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| {
                this.close_settings(window, cx);
                cx.stop_propagation();
            }))
            .w(width)
            .max_h(height)
            .when_some(bounds, |panel, bounds| panel.w(bounds.size.width).h(bounds.size.height).max_h(bounds.size.height))
            .when(components::touch_ui(), |panel| panel.w_full().max_h(gpui::relative(0.8)))
            .overflow_hidden()
            .track_focus(&self.focus)
            .tab_group()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_settings(window, cx);
                }
                cx.stop_propagation();
            }))
            .child(div().id("reader-dictionary-settings-scroll").min_h_0().overflow_y_scroll().child(settings))
            .focus_trap("dictionary-settings-focus", &self.focus);
        let scrim = components::modal_scrim("dictionary-settings-scrim", theme)
            .when(!components::touch_ui(), |scrim| scrim.bg(gpui::transparent_black()))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| {
                this.close_settings(window, cx);
                cx.stop_propagation();
            }))
            .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)));
        let overlay = if components::touch_ui() {
            components::anchor_to_window(components::bottom_sheet_overlay().child(scrim).child(panel), window)
        } else if let Some(bounds) = bounds {
            components::anchor_to_window(components::absolute_full_size().child(scrim).child(gpui::anchored().anchor(Anchor::TopLeft).position(bounds.origin).child(panel)), window)
        } else {
            components::centered_modal(scrim, panel, window)
        };
        gpui::deferred(overlay).with_priority(LOOKUP_POPUP_PRIORITY)
    }

    fn wikipedia_eligible(&self) -> bool {
        !self.selection_only
            && can_lookup_wikipedia(&self.query)
            && (self.translation.is_none() || !self.languages.is_empty())
            && self.wikipedia_request.is_some()
    }

    fn lookup_entry_on_wikipedia(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_only {
            return;
        }
        let Some(request) = self.wikipedia_request.clone() else { return };
        self.reset_result_wheel();
        let Some((_, definition)) = self.dictionary_entries().nth(self.dictionary_index) else { return };
        let query = definition.word.trim().to_owned();
        let popup = cx.new(|cx| {
            let mut popup = Self::empty(query, self.languages.clone(), self.anchor, window, cx);
            popup.popup_area = self.popup_area;
            popup.popup_height = self.popup_height;
            popup.result = Some(Ok(DictionaryLookup { dictionaries: Vec::new(), enabled_count: 0, errors: Vec::new() }));
            popup.wikipedia_request = Some(request);
            popup.wikipedia_page.context = self.wikipedia_page.context.clone();
            popup.wikipedia_page.reuse_reader = self.wikipedia_page.reuse_reader;
            popup.refresh_wikipedia(cx);
            popup
        });
        cx.subscribe_in(&popup, window, |this, _, _: &Closed, window, cx| {
            this.wikipedia_popup = None;
            this.focus.focus(window, cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&popup, |_, _, article: &WikipediaArticle, cx| cx.emit(article.clone())).detach();
        self.wikipedia_popup = Some(popup);
        cx.notify();
    }

    fn visible_wikipedia(&self) -> Option<&WikipediaArticle> {
        self.wikipedia.as_ref().filter(|_| self.wikipedia_eligible())
    }

    fn dictionary_entries(&self) -> impl Iterator<Item = (&str, &DictionaryDefinition)> {
        self.result
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .into_iter()
            .flat_map(|lookup| &lookup.dictionaries)
            .flat_map(|dictionary| dictionary.definitions.iter().map(move |definition| (dictionary.dictionary.as_str(), definition)))
    }

    fn content_loading(&self, cx: &App) -> bool {
        if self.showing_translation() {
            self.translation.as_ref().is_some_and(|translation| translation.read(cx).is_loading())
        } else {
            self.wikipedia_loading || self.wikipedia_equivalents.is_loading() || (!self.selection_only && self.result.is_none())
        }
    }

    fn showing_translation(&self) -> bool {
        self.translation.is_some()
            && (self.show_translation || !self.wikipedia_eligible() || (self.wikipedia_attempted && !self.wikipedia_loading && self.wikipedia_failed.is_none() && self.wikipedia.is_none() && self.wikipedia_hits.is_empty()))
    }

    fn result_position(&self) -> (usize, usize, bool) {
        if self.selection_only {
            return (0, 0, false);
        }
        let dictionary_total = self.dictionary_entries().count();
        let wikipedia_total = self.wikipedia_result_count();
        if self.translation.is_some() {
            let index = if self.showing_translation() { 0 } else { self.wikipedia_index + 1 };
            (wikipedia_total + 1, index, false)
        } else {
            let index = if self.showing_wikipedia() { dictionary_total + self.wikipedia_index } else { self.dictionary_index };
            (dictionary_total + wikipedia_total, index, false)
        }
    }

    fn wikipedia_result_count(&self) -> usize {
        if self.wikipedia_eligible() {
            self.wikipedia_hits.len().max(usize::from(self.wikipedia.is_some() || self.wikipedia_loading || self.wikipedia_failed.is_some()))
        } else {
            0
        }
    }

    fn showing_wikipedia(&self) -> bool {
        let dictionary_total = self.dictionary_entries().count();
        !self.showing_translation() && (dictionary_total == 0 || (self.dictionary_index == dictionary_total && self.wikipedia_result_count() > 0))
    }

    fn refresh_wikipedia(&mut self, cx: &mut Context<Self>) {
        if !self.wikipedia_eligible() {
            self.wikipedia_page.close();
            if self.wikipedia_loading {
                self.wikipedia_attempted = false;
            }
            self._wikipedia_task = Task::ready(());
            self.wikipedia_loading = false;
            // Availability changes hide cached results without discarding
            // them. Restoring a clean miss must not require another request.
            return;
        }
        // Notifications and repainting cannot retry the initial lookup.
        if self.wikipedia_attempted {
            return;
        }
        self.wikipedia_attempted = true;
        if self.wikipedia_hits.is_empty() {
            self.start_wikipedia(self.query.clone(), self.languages.iter().filter_map(|tag| parse_language_tag(tag)).collect(), cx);
        } else {
            self.wikipedia_hit(self.wikipedia_index, cx);
        }
    }

    fn wikipedia_hit(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.wikipedia_eligible() || self.wikipedia_loading {
            return;
        }
        let Some(hit) = self.wikipedia_hits.get(index).cloned() else {
            return;
        };
        self.wikipedia_page.close();
        self.wikipedia_index = index;
        self.definitions_scroll.set_offset(Point::default());
        // Keep search metadata and the retry state when a summary is unavailable.
        let cached = self.wikipedia_cache.get(&hit.title);
        let needs_fetch = cached.is_none();
        self.wikipedia_failed = cached.and_then(|result| result.as_ref().err()).cloned();
        let article = cached.and_then(|result| result.as_ref().ok()).and_then(Option::as_ref).cloned().unwrap_or_else(|| WikipediaArticle { title: hit.title.clone(), extract: hit.description, url: hit.url, language: hit.language.clone() });
        self.wikipedia = Some(article);
        if !self.showing_translation() && !needs_fetch {
            self.popup_height = None;
        }
        self.refresh_wikipedia_equivalents(cx);
        if needs_fetch {
            self.start_wikipedia(hit.title, vec![hit.language], cx);
        }
        cx.notify();
    }

    fn navigate_result(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_result_wheel();
        let (total, index, _) = self.result_position();
        if total < 2 {
            return;
        }
        let index = (index as isize + delta).rem_euclid(total as isize) as usize;
        if self.translation.is_some() {
            self.show_translation = index == 0;
            self.definitions_scroll.set_offset(Point::default());
            if !self.show_translation && !self.wikipedia_hits.is_empty() && index - 1 != self.wikipedia_index {
                self._wikipedia_task = Task::ready(());
                self.wikipedia_loading = false;
                self.wikipedia_hit(index - 1, cx);
            }
            cx.notify();
        } else {
            let dictionary_total = self.dictionary_entries().count();
            self.definitions_scroll.set_offset(Point::default());
            if index >= dictionary_total {
                // The slot after the dictionary entries selects the Wikipedia group.
                self.dictionary_index = dictionary_total;
                let wikipedia_index = index - dictionary_total;
                if !self.wikipedia_hits.is_empty() && wikipedia_index != self.wikipedia_index {
                    self._wikipedia_task = Task::ready(());
                    self.wikipedia_loading = false;
                    self.wikipedia_hit(wikipedia_index, cx);
                }
            } else {
                self.dictionary_index = index;
            }
            cx.notify();
        }
        // Keep focus in the popup while the selected article is being fetched.
        if self.wikipedia_loading {
            self.focus.focus(window, cx);
        }
    }

    fn reset_result_wheel(&mut self) {
        self.wheel_debounce.take();
        self.wheel_delta = 0.0;
        self.wheel_scrolling_text = false;
    }

    fn debounce_result_wheel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        super::input::debounce_wheel(&mut self.wheel_debounce, window, cx, |this, window, cx| {
            this.wheel_debounce.take();
            let delta = std::mem::take(&mut this.wheel_delta);
            let scrolling = std::mem::take(&mut this.wheel_scrolling_text);
            if !scrolling && delta != 0.0 {
                this.navigate_result(if delta < 0.0 { 1 } else { -1 }, window, cx);
            }
        });
    }

    fn handle_result_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        self._dismiss_task = Task::ready(());
        let (total, _, loading) = self.result_position();
        if loading || total < 2 {
            return;
        }
        if self.wheel_scrolling_text {
            self.debounce_result_wheel(window, cx);
            return;
        }
        if let ScrollDelta::Pages(delta) = event.delta {
            let delta = if delta.y != 0.0 { delta.y } else { delta.x };
            if delta.is_finite() && delta != 0.0 {
                self.navigate_result(if delta < 0.0 { 1 } else { -1 }, window, cx);
            }
        } else if matches!(event.delta, ScrollDelta::Pixels(_)) || self.touchpad.is_active() || matches!(event.delta, ScrollDelta::Lines(delta) if delta.x.abs() > delta.y.abs()) {
            self.wheel_delta = 0.0;
            self.wheel_debounce.take();
            if let Some(travel) = self.touchpad.update(event) {
                let delta = if travel.y != 0.0 { travel.y } else { travel.x };
                self.navigate_result(if delta < 0.0 { 1 } else { -1 }, window, cx);
            }
        } else if let ScrollDelta::Lines(delta) = event.delta {
            if delta.y.is_finite() && delta.y != 0.0 {
                self.wheel_delta += delta.y;
                self.debounce_result_wheel(window, cx);
            }
        }
    }

    fn handle_definition_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        self._dismiss_task = Task::ready(());
        let delta = match event.delta {
            ScrollDelta::Pixels(delta) => point(f32::from(delta.x), f32::from(delta.y)),
            ScrollDelta::Lines(delta) | ScrollDelta::Pages(delta) => delta,
        };
        let offset = self.definitions_scroll.offset().y;
        let maximum = self.definitions_scroll.max_offset().y;
        let can_scroll = delta.y < 0.0 && offset > -maximum || delta.y > 0.0 && offset < px(0.0);
        if delta.x.is_finite() && delta.y.is_finite() && delta.y.abs() >= delta.x.abs() && maximum > px(0.0) && (can_scroll || self.wheel_scrolling_text) {
            self.wheel_scrolling_text = true;
            self.wheel_delta = 0.0;
            if matches!(event.delta, ScrollDelta::Pixels(_)) || self.touchpad.is_active() {
                self.touchpad.consume();
            }
            self.debounce_result_wheel(window, cx);
            return;
        }
        self.handle_result_wheel(event, window, cx);
        cx.stop_propagation();
    }

    fn retry_wikipedia(&mut self, cx: &mut Context<Self>) {
        if self.wikipedia_loading || !self.wikipedia_eligible() {
            return;
        }
        if let Some(hit) = self.wikipedia_hits.get(self.wikipedia_index) {
            self.wikipedia_cache.remove(&hit.title);
            self.wikipedia_hit(self.wikipedia_index, cx);
        } else {
            self.wikipedia_attempted = false;
            self.refresh_wikipedia(cx);
        }
    }

    fn start_wikipedia(&mut self, query: String, languages: Vec<LanguageTag>, cx: &mut Context<Self>) {
        let Some(request) = self.wikipedia_request.clone() else { return };
        self.wikipedia_loading = true;
        self.wikipedia_failed = None;
        self._wikipedia_task = cx.spawn(async move |this, cx| {
            let result = request(query.clone(), languages).await;
            let _ = this.update(cx, |this, cx| {
                this.wikipedia_loading = false;
                if !this.showing_translation() {
                    this.popup_height = None;
                }
                if this.wikipedia_eligible() {
                    if !this.wikipedia_hits.is_empty() {
                        let cached = match &result {
                            Some(WikipediaLookup::Article(article)) => Ok(Some(article.clone())),
                            Some(WikipediaLookup::Unavailable(error)) => Err(error.clone()),
                            _ => Ok(None),
                        };
                        this.wikipedia_cache.insert(query, cached);
                    }
                    match result {
                        Some(WikipediaLookup::Article(article)) => {
                            this.wikipedia = Some(article);
                        }
                        Some(WikipediaLookup::Choices(hits)) if this.wikipedia_hits.is_empty() => {
                            this.wikipedia_hits = hits;
                            this.wikipedia_hit(0, cx);
                        }
                        Some(WikipediaLookup::Unavailable(error)) => this.wikipedia_failed = Some(error),
                        _ => {}
                    }
                }
                this.refresh_wikipedia_equivalents(cx);
                cx.notify();
            });
        });
        cx.notify();
    }

    pub(crate) fn set_anchor(&mut self, anchor: Option<Bounds<Pixels>>, cx: &mut Context<Self>) {
        if self.anchor != anchor {
            self.anchor = anchor;
            if let Some(popup) = &self.wikipedia_popup {
                popup.update(cx, |popup, cx| popup.set_anchor(anchor, cx));
            }
            cx.notify();
        }
    }

    /// Place after the document so its new layout is available before reading
    /// selection geometry. Only changed bounds request another popup frame.
    pub(crate) fn track_anchor(popup: WeakEntity<Self>, read_bounds: impl FnOnce(&App) -> Option<Bounds<Pixels>> + 'static) -> impl IntoElement {
        canvas(
            |_, _, _| (),
            move |_, _, _, cx| {
                let bounds = read_bounds(cx);
                cx.defer(move |cx| {
                    let _ = popup.update(cx, |popup, cx| popup.set_anchor(bounds, cx));
                });
            },
        )
        .absolute()
        .inset_0()
    }

    pub(crate) fn show_selection(&mut self, text: String, anchor: Option<Bounds<Pixels>>, cx: &mut Context<Self>) {
        self.reset_result_wheel();
        self._dismiss_task = Task::ready(());
        self._task = Task::ready(());
        self.query = text;
        self.definitions_scroll.set_offset(Point::default());
        self.anchor = anchor;
        self.selection_only = true;
        self.settings = None;
        self.wikipedia_request = None;
        self.refresh_wikipedia(cx);
        cx.notify();
    }
}

impl Focusable for DictionaryPopup {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DictionaryPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(article) = self.wikipedia_page.render(&self.focus, window, cx) {
            return article;
        }
        if let Some(popup) = self.wikipedia_popup.clone() {
            return popup.into_any_element();
        }
        // Do not paint a small loading popup before its initial content arrives.
        // Once shown, keep the popup available while switching or retrying results.
        if self.popup_area.is_none() && self.content_loading(cx) {
            return div().track_focus(&self.focus).on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                    cx.stop_propagation();
                }
            })).into_any_element();
        }
        if self.focus.contains_focused(window, cx) {
            let focused = window.focused(cx);
            // Check after the new focus tree is drawn. A removed control must
            // not strand focus, while an opened popover keeps its own focus.
            cx.defer_in(window, move |this, window, cx| {
                if window.focused(cx) == focused && !this.focus.contains_focused(window, cx) {
                    this.focus.focus(window, cx);
                }
            });
        }
        if let Some(settings) = self.settings.clone() {
            return self.render_settings(settings, window, cx).into_any_element();
        }
        let theme = components::browser_theme(cx);
        let dictionary_total = if self.selection_only { 0 } else { self.dictionary_entries().count() };
        let last_dictionary_index = if self.wikipedia_result_count() > 0 { dictionary_total } else { dictionary_total.saturating_sub(1) };
        let dictionary_index = self.dictionary_index.min(last_dictionary_index);
        if self.dictionary_index != dictionary_index {
            self.dictionary_index = dictionary_index;
            self.definitions_scroll.set_offset(Point::default());
        }
        let mut title = self.query.trim_start().to_owned();
        let showing_translation = self.showing_translation();
        let showing_wikipedia = self.showing_wikipedia();
        let wikipedia = self.visible_wikipedia().filter(|_| showing_wikipedia);
        let mut definitions = div().id("lookup-definition-content").min_w_0().flex_none().flex().flex_col().gap(px(12.0)).on_scroll_wheel(cx.listener(Self::handle_definition_wheel));
        if showing_translation {
            definitions = definitions.children(self.translation.clone());
        } else if self.selection_only {
            definitions = definitions.child(TextView::plain("lookup-selected-text", self.query.trim_start().to_owned()).selectable(true));
        } else if !showing_wikipedia || dictionary_total == 0 {
            match &self.result {
                None => definitions = definitions.child(text!(reader::looking_up)),
                Some(Err(_)) => definitions = definitions.child(text!(reader::dictionary_lookup_failed)),
                Some(Ok(result)) => {
                    if dictionary_total == 0 && self.downloading {
                        definitions = definitions.child(text!(reader::downloading_a_dictionary_results_will_update_when_it_is_installed));
                    } else if dictionary_total == 0 && self.wikipedia_eligible() && self.wikipedia_attempted && self.wikipedia_failed.is_none() && wikipedia.is_none() && !self.wikipedia_loading {
                        definitions = definitions.child(text!(reader::no_wikipedia_article_found));
                    } else if self.translation.is_none() && result.enabled_count == 0 && self.wikipedia_failed.is_none() && wikipedia.is_none() && !self.wikipedia_loading {
                        definitions = definitions.child(text!(reader::open_dictionary_settings_to_download_import_or_enable_a_dictionary_for_offline));
                    } else if self.translation.is_none() && dictionary_total == 0 && self.wikipedia_failed.is_none() && wikipedia.is_none() && !self.wikipedia_loading {
                        definitions = definitions.child(text!(reader::no_definition_found_in_the_enabled_dictionaries));
                    }
                    if let Some((_, definition)) = self.dictionary_entries().nth(self.dictionary_index) {
                        title = definition.word.trim_start().to_owned();
                        let entry = div().w_full().min_w_0().flex_none().debug_selector(|| "dictionary-entry".into());
                        definitions = definitions.child(entry.child(TextView::plain("lookup-definition-text", definition_text(&definition.text)).selectable(true)));
                    }
                    if let Some(error) = result.errors.first() {
                        definitions = definitions.child(div().text_color(theme.text_muted).child(components::user_facing_error(error, text!(reader::dictionary_partly_unavailable))));
                    }
                }
            }
        }
        if showing_wikipedia && self.wikipedia_eligible() && self.wikipedia_failed.is_some() {
            definitions = definitions.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.wikipedia_failed.as_ref().map_or(text!(reader::wikipedia_unavailable), |error| components::user_facing_error(error, text!(reader::wikipedia_unavailable))))
                    .child(components::outlined_text_button("wikipedia-retry", text!(common::retry)).on_click(cx.listener(|this, _, _, cx| this.retry_wikipedia(cx)))),
            );
        }
        if let Some(article) = wikipedia {
            if let Some(equivalents) = self.wikipedia_equivalents.render(cx) {
                definitions = definitions.child(equivalents);
            }
            if self.translation.is_none() {
                definitions = definitions.child(div().min_w_0().flex_none().debug_selector(|| "wikipedia-summary".into()).child(TextView::plain("lookup-wikipedia-text", article.extract.trim_start().to_owned()).selectable(true)));
            }
        }
        if self.selection_only {
            title = text!(reader::selected_text).to_owned();
        } else if let Some(article) = wikipedia {
            title = article.title.trim_start().to_owned();
        }
        let (total, index, loading) = self.result_position();
        let counter = if total > 1 {
            let mut choices = self.dictionary_entries().map(|(_, definition)| definition.word.clone()).collect::<Vec<_>>();
            if self.wikipedia_result_count() > 0 {
                if self.wikipedia_hits.is_empty() {
                    choices.push(self.wikipedia.as_ref().map_or_else(|| self.query.clone(), |article| article.title.clone()));
                } else {
                    choices.extend(self.wikipedia_hits.iter().map(|hit| hit.title.clone()));
                }
            }
            if self.translation.is_some() {
                choices.insert(0, text!(reader::mymemory).to_owned());
            }
            let owner = cx.weak_entity();
            let menu_state = window.use_keyed_state("lookup-results-menu", cx, |_, _| None::<Entity<ListState<ResultList>>>);
            let closing_state = menu_state.clone();
            let bounds = self.picker_bounds();
            let counter = Popover::new("lookup-results")
                .anchor(gpui::Anchor::BottomLeft)
                .when(components::touch_ui(), |popover| popover.anchor(Anchor::TopLeft).position(point(px(0.0), px(0.0))).overlay_closable(false))
                .when_some(bounds.filter(|_| !components::touch_ui()), |popover, bounds| popover.anchor(Anchor::TopLeft).position(bounds.origin))
                .appearance(false)
                .on_open_change(move |open, _, cx| {
                    if !open {
                        closing_state.update(cx, |menu, _| *menu = None);
                    }
                })
                .trigger(components::outlined_text_button("lookup-counter", format!("{}/{}", index + 1, total)).ghost().no_fill().w_full().disabled(loading).debug_selector(|| "lookup-counter".into()))
                .content(move |_, window, cx| {
                    let cached = menu_state
                        .read(cx)
                        .as_ref()
                        .filter(|list| {
                            let results = list.read(cx).delegate();
                            results.titles == choices && results.current == index
                        })
                        .cloned();
                    let menu = match cached {
                        Some(menu) => menu,
                        None => {
                            let menu = cx.new(|cx| {
                                let mut list = ListState::new(ResultList { filtered: (0..choices.len()).collect(), titles: choices.clone(), current: index }, window, cx).searchable(true);
                                list.set_selected_index(Some(IndexPath::new(index)), window, cx);
                                list
                            });
                            let popover = cx.entity();
                            let owner = owner.clone();
                            window
                                .subscribe(&menu, cx, move |menu, event: &ListEvent, window, cx| {
                                    match event {
                                        ListEvent::Confirm(choice) => {
                                            let Some(choice) = menu.read(cx).delegate().filtered.get(choice.row).copied() else { return };
                                            let _ = owner.update(cx, |popup, cx| {
                                                let (_, current, _) = popup.result_position();
                                                popup.navigate_result(choice as isize - current as isize, window, cx);
                                                popup.focus.focus(window, cx);
                                            });
                                        }
                                        ListEvent::Cancel => {
                                            let _ = owner.update(cx, |popup, cx| popup.focus.focus(window, cx));
                                        }
                                        _ => return,
                                    }
                                    popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                                })
                                .detach();
                            menu.update(cx, |list, cx| list.focus(window, cx));
                            menu_state.update(cx, |state, _| *state = Some(menu.clone()));
                            menu
                        }
                    };
                    if components::touch_ui() {
                        return mobile_picker(&menu, text!(reader::filter_results), window, cx);
                    }
                    desktop_picker(&menu, text!(reader::filter_results), bounds, window, cx)
                });
            div().flex_1().min_w_0().child(counter).into_any_element()
        } else {
            div().debug_selector(|| "lookup-counter".into()).flex_1().text_center().child(format!("{}/{}", index + 1, total)).into_any_element()
        };
        let mut heading = div().w_full().min_w_0().flex_none().flex().flex_col().gap(px(8.0));
        if total > 1 {
            heading = heading.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        components::outlined_icon_button("lookup-previous", text!(reader::previous_result), IconName::ChevronLeft)
                            .no_fill()
                            .debug_selector(|| "lookup-previous".into())
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, window, cx| this.navigate_result(-1, window, cx))),
                    )
                    .child(counter)
                    .child(
                        components::outlined_icon_button("lookup-next", text!(reader::next_result), IconName::ChevronRight)
                            .no_fill()
                            .debug_selector(|| "lookup-next".into())
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, window, cx| this.navigate_result(1, window, cx))),
                    ),
            );
        }
        let settings = (!self.selection_only && self.backend.is_some()).then(|| {
            components::outlined_icon_button("dictionary-settings", text!(reader::dictionary_settings), IconName::Settings)
                .no_fill()
                .debug_selector(|| "dictionary-settings".into())
                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
        });
        let close = (self.translation.is_some() && !components::touch_ui()).then(|| components::outlined_icon_button("translation-close", text!(reader::close_translation), IconName::Close).no_fill().on_click(cx.listener(|_, _, _, cx| cx.emit(Closed))));
        let show_wikipedia_button = !showing_wikipedia && self.wikipedia_request.is_some() && self.dictionary_entries().nth(self.dictionary_index).is_some_and(|(_, definition)| can_lookup_wikipedia(&definition.word));
        let wikipedia_lookup = show_wikipedia_button.then(|| {
            components::outlined_icon_button("lookup-wikipedia", text!(reader::look_up_on_wikipedia), IconName::Globe)
                .no_fill()
                .debug_selector(|| "lookup-wikipedia".into())
                .on_click(cx.listener(|this, _, window, cx| this.lookup_entry_on_wikipedia(window, cx)))
        });
        let translation_actions = self.translation.as_ref().filter(|_| showing_translation).map(|translation| translation.update(cx, |translation, cx| translation.header_actions(window, cx)));
        let title = components::section_title(title).flex_1().min_w_0();
        if let Some(article) = wikipedia {
            let full_article = article.clone();
            let title_url = article.url.clone();
            heading = heading.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        // The adjacent browser button provides this same keyboard action.
                        components::base_button("wikipedia-title")
                            .tab_stop(false)
                            .ghost()
                            .no_fill()
                            .h_auto()
                            .p_0()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_normal()
                            .justify_start()
                            .debug_selector(|| "wikipedia-title".into())
                            .tooltip(text!(reader::open_article_on_wikipedia))
                            .child(title)
                            .on_click(move |_, _, cx| cx.open_url(&title_url)),
                    )
                    .child(
                        components::outlined_icon_button("wikipedia-full-article", text!(reader::full_article), IconName::BookOpen)
                            .no_fill()
                            .disabled(self.wikipedia_loading)
                            .on_click(cx.listener(move |popup, _, window, cx| popup.open_wikipedia_article(full_article.clone(), window, cx))),
                    )
                    .children(source::actions(Some(&article.url), SourcePlacement::Lookup, window, cx))
                    .children(settings)
                    .children(close),
            );
        } else {
            heading = heading.child(div().flex().items_center().gap(px(8.0)).child(div().flex_1().min_w_0().overflow_hidden().child(title.truncate().line_clamp(1))).children(translation_actions).children(wikipedia_lookup).children(settings).children(close));
        }
        // Keep the container in the tab order when no child controls exist.
        self.focus = self.focus.clone().tab_stop((self.selection_only || self.backend.is_none()) && wikipedia.is_none() && total <= 1 && !show_wikipedia_button).tab_index(0);
        let definitions = div().id("dictionary-definitions").debug_selector(|| "dictionary-definitions".into()).track_scroll(&self.definitions_scroll).min_w_0().min_h_0().overflow_y_scroll().child(definitions);
        self.render_popup(heading.into_any_element(), definitions.into_any_element(), window, cx).into_any_element()
    }
}

impl DictionaryPopup {
    fn picker_bounds(&self) -> Option<Bounds<Pixels>> {
        let (mut area, corner) = self.popup_area?;
        let height = self.popup_height.unwrap_or(area.size.height);
        if corner == Anchor::BottomLeft {
            area.origin.y = area.bottom() - height;
        }
        area.size.height = height;
        Some(area)
    }

    fn render_popup(&mut self, heading: AnyElement, body: AnyElement, window: &Window, cx: &mut Context<Self>) -> gpui::Deferred {
        let theme = components::browser_theme(cx);
        let insets = window.insets().effective();
        let left = insets.left + px(12.0);
        let right = (window.viewport_size().width - insets.right - px(12.0)).max(left + px(1.0));
        let top = insets.top + px(12.0);
        let bottom = (window.viewport_size().height - insets.bottom - px(12.0)).max(top + px(1.0));
        let anchor = self.anchor.filter(|_| !components::touch_ui() && !components::has_native_back_button());
        let (area, corner) = *self.popup_area.get_or_insert_with(|| {
            let width = (right - left).min(window.rem_size() * LOOKUP_POPUP_WIDTH_REM);
            let available = bottom - top;
            let gap = px(8.0 + 1.0 / window.scale_factor());
            let placement = anchor.and_then(|bounds| {
                let above = (bounds.top() - gap - top).max(px(0.0));
                let below = (bottom - bounds.bottom() - gap).max(px(0.0));
                let place_above = below < px(160.0) && above > below;
                let room = if place_above { above } else { below };
                (room >= px(160.0)).then_some((bounds, place_above, room))
            });
            let height = placement.map_or(available, |(_, _, room)| room).min(px(LOOKUP_POPUP_MAX_HEIGHT));
            let corner = Anchor::TopLeft;
            let origin = if let Some((bounds, above, _)) = placement {
                point(bounds.left().clamp(left, right - width), if above { bounds.top() - gap - height } else { bounds.bottom() + gap })
            } else {
                point(left + (right - left - width) / 2.0, top + (available - height) / 2.0)
            };
            (Bounds::new(origin, size(width, height)), corner)
        });
        let height = area.size.height.min((bottom - area.origin.y).max(px(1.0)));
        let pending_height = self.popup_height.filter(|_| self.content_loading(cx));
        let selector = if self.translation.is_some() { "translation-popup" } else { "dictionary-popup" };
        // Selectable TextViews need mouse-down to reach the window selection controller.
        // Occlusion and the click guard contain interaction with the document underneath.
        let panel = (if components::touch_ui() { components::bottom_sheet_surface(theme) } else { components::modal_surface(theme) })
            .font_family(Theme::global(cx).font_family.clone())
            .text_size(Theme::global(cx).font_size)
            .id("lookup-popup-surface")
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| {
                cx.emit(Closed);
                cx.stop_propagation();
            }))
            .p(px(components::SPACE_MD))
            .flex()
            .flex_col()
            .gap(px(components::SPACE_MD))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this._dismiss_task = Task::ready(())))
            .on_click(|_, _, cx| cx.stop_propagation())
            .debug_selector(move || selector.into())
            .w(area.size.width)
            .max_w((right - area.origin.x).max(px(1.0)))
            .max_h(height)
            .when_some(pending_height.filter(|_| !components::touch_ui()), |panel, previous| panel.h(previous.min(height)))
            .when(components::touch_ui(), |panel| panel.w_full().max_w_full().max_h(gpui::relative(0.8)))
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                if !this.definitions_scroll.bounds().contains(&event.position) {
                    this.handle_result_wheel(event, window, cx);
                }
                cx.stop_propagation();
            }))
            .key_context("DictionaryLookup")
            .track_focus(&self.focus)
            .tab_group()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => cx.emit(Closed),
                    "left" if !event.keystroke.modifiers.modified() => this.navigate_result(-1, window, cx),
                    "right" if !event.keystroke.modifiers.modified() => this.navigate_result(1, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(heading)
            .child(body)
            .focus_trap("dictionary-focus", &self.focus);
        // Keep the selected word visible while containing pointer input, so a
        // dismissal cannot turn a page or begin another text selection.
        let movement_target = cx.entity().downgrade();
        let scrim = components::absolute_full_size()
            .id("dictionary-scrim")
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| {
                cx.emit(Closed);
                cx.stop_propagation();
            }))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this._dismiss_task = Task::ready(());
                    this.element_click = WordClick::default();
                    if !this.selection_only && this.translation.is_none() && this.anchor.is_some_and(|bounds| bounds.contains(&event.position)) {
                        this.element_click.begin(event);
                    }
                    cx.stop_propagation();
                }),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = movement_target.update(cx, |this, _| {
                                    this.element_click.moved(event.position);
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                let expand = this.element_click.finish_click(event);
                // A single click dismisses after the multi-click interval;
                // another press cancels it so double/fourth clicks can expand.
                if !this.selection_only && this.translation.is_none() && event.mouse_position().is_some() && event.modifiers() == Default::default() && this.anchor.is_some_and(|bounds| bounds.contains(&event.position())) {
                    if matches!(event.click_count(), 2 | 4) {
                        if expand {
                            cx.emit(SelectElement { position: event.position() });
                        }
                        return;
                    }
                    if matches!(event.click_count(), 1 | 3) {
                        let delay = cx.background_executor().timer(window.double_click_interval());
                        this._dismiss_task = cx.spawn(async move |this, cx| {
                            delay.await;
                            let _ = this.update(cx, |_, cx| cx.emit(Closed));
                        });
                        return;
                    }
                }
                cx.emit(Closed);
            }));
        if components::touch_ui() {
            let overlay = components::anchor_to_window(components::bottom_sheet_overlay().child(scrim.bg(theme.overlay_scrim)).child(panel), window);
            return gpui::deferred(overlay).with_priority(LOOKUP_POPUP_PRIORITY);
        }
        let target = cx.entity().downgrade();
        let panel = div().child(panel).on_children_prepainted(move |bounds, _, cx| {
            if let Some(bounds) = bounds.first() {
                let bounds = *bounds;
                let target = target.clone();
                cx.defer(move |cx| {
                    let _ = target.update(cx, |popup, cx| {
                        popup.popup_height = Some(bounds.size.height);
                        if let Some(translation) = &popup.translation {
                            translation.update(cx, |translation, cx| {
                                if translation.picker_bounds != Some(bounds) {
                                    translation.picker_bounds = Some(bounds);
                                    cx.notify();
                                }
                            });
                        }
                    });
                });
            }
        });
        // Keep the top-left fixed as results change height or nested screens open.
        let position = if corner == Anchor::BottomLeft { point(area.left(), area.bottom()) } else { area.origin };
        let panel = gpui::anchored().anchor(corner).position(position).snap_to_window().child(panel);
        let overlay = components::anchor_to_window(components::absolute_full_size().child(scrim).child(panel), window);
        gpui::deferred(overlay).with_priority(LOOKUP_POPUP_PRIORITY)
    }
}

#[cfg(test)]
#[path = "dictionary_tests.rs"]
mod tests;
