//! Shared in-app definition popup for EPUB and PDF selections.
use ui_text::text;

use std::{collections::BTreeMap, future::Future, pin::Pin, rc::Rc};

use content_address::BookId;
use dictionary::{Client as DictionaryClient, Definition as DictionaryDefinition, DictionaryLookup};
use dictionary_settings::{Closed as DictionarySettingsClosed, DictionarySettings};
use gpui::prelude::*;
use gpui::{
    Anchor, AnyElement, App, Bounds, ClickEvent, Context, DispatchPhase, Edges, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollDelta, ScrollHandle, ScrollWheelEvent, Task, WeakEntity, Window, anchored, canvas, div, point, px,
};
use gpui_component::popover::Popover;
use gpui_component::text::TextView;
use gpui_component::{Disableable, FocusTrapElement, IconName};
use html::selection::WORD_SELECTION_DRAG_DISTANCE;
use language_tags::LanguageTag;
use reader_lookups::{WikipediaArticle, WikipediaChoice, WikipediaLookup, parse_language_tag};
use ui_components as components;

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
    backend: Option<Rc<dyn DictionaryClient>>,
    settings: Option<Entity<DictionarySettings>>,
    query: String,
    result: Option<Result<DictionaryLookup, String>>,
    dictionary_index: usize,
    definitions_scroll: ScrollHandle,
    wheel_delta: f32,
    wheel_debounce: Option<Task<()>>,
    wheel_scrolling_text: bool,
    touchpad: components::TouchpadNavigation,
    downloading: bool,
    selection_only: bool,
    anchor: Option<Bounds<Pixels>>,
    element_click: WordClick,
    focus: FocusHandle,
    wikipedia_request: Option<WikipediaRequest>,
    wikipedia_popup: Option<Entity<Self>>,
    wikipedia: Option<WikipediaArticle>,
    languages: Vec<String>,
    wikipedia_attempted: bool,
    wikipedia_hits: Vec<WikipediaChoice>,
    wikipedia_index: usize,
    wikipedia_cache: BTreeMap<String, Option<WikipediaArticle>>,
    wikipedia_loading: bool,
    _dismiss_task: Task<()>,
    _wikipedia_task: Task<()>,
    _task: Task<()>,
}

impl EventEmitter<Closed> for DictionaryPopup {}
impl EventEmitter<SelectElement> for DictionaryPopup {}

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
                        this.result = Some(result);
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
            element_click: WordClick::default(),
            focus,
            wikipedia_request: None,
            wikipedia_popup: None,
            wikipedia: None,
            languages,
            wikipedia_attempted: false,
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
        cx.subscribe(&translation, |_, _, _: &Closed, cx| cx.emit(Closed)).detach();
        Self { translation: Some(translation), selection_only: true, ..Self::empty(query, languages, anchor, window, cx) }
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
        let theme = components::browser_theme(cx);
        let insets = window.insets().effective();
        let width = (window.viewport_size().width - insets.left - insets.right - px(32.0)).clamp(px(1.0), px(640.0));
        let height = (window.viewport_size().height - insets.top - insets.bottom - px(32.0)).clamp(px(1.0), px(640.0));
        let panel = components::modal_form_surface(theme)
            .debug_selector(|| "reader-dictionary-settings".into())
            .w(width)
            .max_h(height)
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
        let scrim = components::modal_scrim("dictionary-settings-scrim", theme).on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)));
        gpui::deferred(components::centered_modal(scrim, panel, window)).with_priority(10)
    }

    fn wikipedia_eligible(&self) -> bool {
        !self.selection_only && !self.downloading && self.result.as_ref().is_some_and(|result| result.as_ref().is_ok_and(|lookup| lookup.errors.is_empty() && lookup.dictionaries.iter().all(|dictionary| dictionary.definitions.is_empty())))
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
            popup.result = Some(Ok(DictionaryLookup { dictionaries: Vec::new(), enabled_count: 0, errors: Vec::new() }));
            popup.wikipedia_request = Some(request);
            popup.refresh_wikipedia(cx);
            popup
        });
        cx.subscribe_in(&popup, window, |this, _, _: &Closed, window, cx| {
            this.wikipedia_popup = None;
            this.focus.focus(window, cx);
            cx.notify();
        })
        .detach();
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

    fn refresh_wikipedia(&mut self, cx: &mut Context<Self>) {
        if !self.wikipedia_eligible() {
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
        self.start_wikipedia(self.query.clone(), self.languages.iter().filter_map(|tag| parse_language_tag(tag)).collect(), cx);
    }

    fn wikipedia_hit(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.wikipedia_eligible() || self.wikipedia_loading {
            return;
        }
        let Some(hit) = self.wikipedia_hits.get(index).cloned() else {
            return;
        };
        self.wikipedia_index = index;
        self.definitions_scroll.set_offset(Point::default());
        // Search metadata remains useful when the optional summary fails.
        // Cached None marks a failed attempt and prevents retry on navigation.
        let cached = self.wikipedia_cache.get(&hit.title);
        let article = cached.and_then(Clone::clone).unwrap_or_else(|| WikipediaArticle { title: hit.title.clone(), extract: hit.description, url: hit.url, language: hit.language.clone() });
        self.wikipedia = Some(article);
        if cached.is_none() {
            self.wikipedia_cache.insert(hit.title.clone(), None);
            self.start_wikipedia(hit.title, vec![hit.language], cx);
        }
        cx.notify();
    }

    fn navigate_result(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_result_wheel();
        if self.selection_only {
            return;
        }
        let dictionary_total = self.dictionary_entries().count();
        let (index, total) = if dictionary_total > 0 {
            (self.dictionary_index, dictionary_total)
        } else if self.wikipedia_eligible() {
            (self.wikipedia_index, self.wikipedia_hits.len())
        } else {
            (0, 0)
        };
        if total < 2 {
            return;
        }
        let index = (index as isize + delta).rem_euclid(total as isize) as usize;
        if dictionary_total > 0 {
            self.dictionary_index = index;
            self.definitions_scroll.set_offset(Point::default());
            cx.notify();
        } else {
            self.wikipedia_hit(index, cx);
        }
        // Fetching an article disables both arrows. Keep focus in the popup
        // so Escape and Tab still work while the request is pending.
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
        let total = self.dictionary_entries().count() + if self.wikipedia_eligible() { self.wikipedia_hits.len() } else { 0 };
        if self.selection_only || self.translation.is_some() || self.wikipedia_loading || total < 2 {
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

    fn start_wikipedia(&mut self, query: String, languages: Vec<LanguageTag>, cx: &mut Context<Self>) {
        let Some(request) = self.wikipedia_request.clone() else { return };
        self.wikipedia_loading = true;
        self._wikipedia_task = cx.spawn(async move |this, cx| {
            let result = request(query.clone(), languages).await;
            let _ = this.update(cx, |this, cx| {
                this.wikipedia_loading = false;
                if this.wikipedia_eligible() {
                    match result {
                        Some(WikipediaLookup::Article(article)) => {
                            if !this.wikipedia_hits.is_empty() {
                                this.wikipedia_cache.insert(query, Some(article.clone()));
                            }
                            this.wikipedia = Some(article);
                        }
                        Some(WikipediaLookup::Choices(hits)) if this.wikipedia_hits.is_empty() => {
                            this.wikipedia_hits = hits;
                            this.wikipedia_hit(0, cx);
                        }
                        _ => {}
                    }
                }
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
        if let Some(popup) = self.wikipedia_popup.clone() {
            return popup.into_any_element();
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
        if let Some(translation) = self.translation.clone() {
            return self.render_popup(None, translation.into_any_element(), window, cx).into_any_element();
        }
        let theme = components::browser_theme(cx);
        let dictionary_total = if self.selection_only { 0 } else { self.dictionary_entries().count() };
        let dictionary_index = self.dictionary_index.min(dictionary_total.saturating_sub(1));
        if self.dictionary_index != dictionary_index {
            self.dictionary_index = dictionary_index;
            self.definitions_scroll.set_offset(Point::default());
        }
        let mut title = self.query.trim_start().to_owned();
        let wikipedia = self.visible_wikipedia();
        let mut definitions = div().id("lookup-definition-content").min_w_0().flex_none().flex().flex_col().gap(px(12.0)).on_scroll_wheel(cx.listener(Self::handle_definition_wheel));
        if self.selection_only {
            definitions = definitions.child(TextView::plain("lookup-selected-text", self.query.trim_start().to_owned()).selectable(true));
        } else {
            match &self.result {
                None => definitions = definitions.child(text!(reader::looking_up)),
                Some(Err(_)) => definitions = definitions.child(text!(reader::dictionary_lookup_failed)),
                Some(Ok(result)) => {
                    if dictionary_total == 0 && self.downloading {
                        definitions = definitions.child(text!(reader::downloading_a_dictionary_results_will_update_when_it_is_installed));
                    } else if dictionary_total == 0 && self.wikipedia_eligible() && self.wikipedia_attempted && wikipedia.is_none() && !self.wikipedia_loading {
                        definitions = definitions.child(text!(reader::no_wikipedia_article_found));
                    } else if result.enabled_count == 0 && wikipedia.is_none() && !self.wikipedia_loading {
                        definitions = definitions.child(text!(reader::open_dictionary_settings_to_download_import_or_enable_a_dictionary_for_offline));
                    } else if dictionary_total == 0 && wikipedia.is_none() && !self.wikipedia_loading {
                        definitions = definitions.child(text!(reader::no_definition_found_in_the_enabled_dictionaries));
                    }
                    if let Some((_, definition)) = self.dictionary_entries().nth(self.dictionary_index) {
                        title = definition.word.trim_start().to_owned();
                        let entry = div().w_full().min_w_0().flex_none().debug_selector(|| "dictionary-entry".into());
                        definitions = definitions.child(entry.child(TextView::plain("lookup-definition-text", definition_text(&definition.text)).selectable(true)));
                    }
                    if !result.errors.is_empty() {
                        definitions = definitions.child(div().text_color(theme.text_muted).child(text!(reader::dictionary_partly_unavailable)));
                    }
                }
            }
        }
        if self.wikipedia_loading {
            definitions = definitions.child(div().text_color(theme.text_muted).child(text!(reader::looking_up_wikipedia)));
        }
        if let Some(article) = wikipedia {
            definitions = definitions.child(div().min_w_0().flex_none().debug_selector(|| "wikipedia-summary".into()).child(TextView::plain("lookup-wikipedia-text", article.extract.trim_start().to_owned()).selectable(true)));
        }
        if self.selection_only {
            title = text!(reader::selected_text).to_owned();
        } else if let Some(article) = wikipedia {
            title = article.title.trim_start().to_owned();
        }
        let (total, index, loading) = if dictionary_total > 0 {
            (dictionary_total, self.dictionary_index, false)
        } else if self.wikipedia_eligible() {
            (self.wikipedia_hits.len(), self.wikipedia_index, self.wikipedia_loading)
        } else {
            (0, 0, false)
        };
        let mut heading = div().min_w_0().flex_none().flex().flex_col().gap(px(8.0));
        if total > 1 {
            heading = heading.child(
                div()
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
                    .child(div().debug_selector(|| "lookup-counter".into()).flex_1().text_center().child(format!("{}/{}", index + 1, total)))
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
        let show_wikipedia_button = dictionary_total > 0 && self.wikipedia_request.is_some();
        let wikipedia_lookup = show_wikipedia_button.then(|| {
            components::outlined_icon_button("lookup-wikipedia", text!(reader::look_up_on_wikipedia), IconName::Globe)
                .no_fill()
                .debug_selector(|| "lookup-wikipedia".into())
                .on_click(cx.listener(|this, _, window, cx| this.lookup_entry_on_wikipedia(window, cx)))
        });
        let title = components::section_title(title).flex_1().min_w_0().truncate().line_clamp(1);
        if let Some(article) = wikipedia {
            let title_url = article.url.clone();
            let browser_url = article.url.clone();
            let source_url = article.url.clone();
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
                            .whitespace_nowrap()
                            .justify_start()
                            .debug_selector(|| "wikipedia-title".into())
                            .tooltip(text!(reader::open_article_on_wikipedia))
                            .child(title)
                            .on_click(move |_, _, cx| cx.open_url(&title_url)),
                    )
                    .child(
                        components::outlined_icon_button("wikipedia-open-browser", text!(reader::open_wikipedia_in_browser), IconName::Globe)
                            .no_fill()
                            .debug_selector(|| "wikipedia-open-browser".into())
                            .on_click(move |_, _, cx| cx.open_url(&browser_url)),
                    )
                    .child(
                        Popover::new("wikipedia-info")
                            .anchor(gpui::Anchor::TopRight)
                            .appearance(false)
                            .trigger(components::outlined_icon_button("wikipedia-info-button", text!(common::source_and_license), IconName::Info).no_fill().tab_stop(false).debug_selector(|| "wikipedia-info-button".into()))
                            .child(
                                components::modal_form_surface(theme)
                                    .debug_selector(|| "wikipedia-attribution".into())
                                    .w(px(260.0))
                                    .max_w(window.viewport_size().width - px(48.0))
                                    .gap(px(8.0))
                                    .child(components::section_title(text!(common::source_and_license)))
                                    .child(text!(reader::wikipedia_contributors))
                                    .child(components::outlined_text_button("wikipedia-source", text!(reader::wikipedia_article)).ghost().no_fill().on_click(move |_, _, cx| cx.open_url(&source_url)))
                                    .child(components::outlined_text_button("wikipedia-license", text!(reader::cc_by_sa_4_0)).ghost().no_fill().on_click(|_, _, cx| cx.open_url("https://creativecommons.org/licenses/by-sa/4.0/"))),
                            ),
                    )
                    .children(settings),
            );
        } else {
            heading = heading.child(div().flex().items_center().gap(px(8.0)).child(div().flex_1().min_w_0().overflow_hidden().child(title)).children(wikipedia_lookup).children(settings));
        }
        // Keep the container in the tab order when no child controls exist.
        self.focus = self.focus.clone().tab_stop((self.selection_only || self.backend.is_none()) && wikipedia.is_none() && total <= 1 && !show_wikipedia_button).tab_index(0);
        let definitions = div().id("dictionary-definitions").debug_selector(|| "dictionary-definitions".into()).track_scroll(&self.definitions_scroll).min_w_0().min_h_0().overflow_y_scroll().child(definitions);
        self.render_popup(Some(heading.into_any_element()), definitions.into_any_element(), window, cx).into_any_element()
    }
}

impl DictionaryPopup {
    fn render_popup(&self, heading: Option<AnyElement>, body: AnyElement, window: &Window, cx: &mut Context<Self>) -> gpui::Deferred {
        let theme = components::browser_theme(cx);
        let insets = window.insets().effective();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let font_id = window.text_system().resolve_font(&text_style.font());
        // Like CSS 80ch: scale the reading width with the current font, including room for padding and borders.
        let line_width = window.text_system().advance(font_id, font_size, '0').map_or(font_size * 0.5, |advance| advance.width) * 80.0;
        let width = (window.viewport_size().width - insets.left - insets.right - px(24.0)).max(px(1.0)).min(line_width + px(components::SPACE_MD * 2.0 + 2.0));
        let available = window.viewport_size().height - insets.top - insets.bottom - px(24.0);
        let anchor = self.anchor.filter(|_| !components::touch_ui() && !components::has_native_back_button());
        let placement = anchor.and_then(|bounds| {
            let above = (bounds.top() - insets.top - px(20.0)).max(px(1.0));
            let below = (window.viewport_size().height - insets.bottom - bounds.bottom() - px(20.0)).max(px(1.0));
            let place_above = above > below;
            let room = if place_above { above } else { below };
            // Long passages can occupy almost the whole viewport. Keep the
            // actions reachable by centering when neither side has room.
            (room >= px(160.0)).then_some((bounds, place_above, room))
        });
        let height = placement.map_or(available, |(_, _, room)| room).min(available).max(px(1.0));
        let selector = if self.translation.is_some() { "translation-popup" } else { "dictionary-popup" };
        // Selectable TextViews need mouse-down to reach the window selection controller.
        // Occlusion and the click guard contain interaction with the document underneath.
        let panel = components::modal_surface(theme)
            .id("lookup-popup-surface")
            .p(px(components::SPACE_MD))
            .flex()
            .flex_col()
            .gap(px(components::SPACE_MD))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this._dismiss_task = Task::ready(())))
            .on_click(|_, _, cx| cx.stop_propagation())
            .debug_selector(move || selector.into())
            .max_w(width)
            .max_h(height)
            .when(self.translation.is_some(), |panel| panel.w(width.min(px(420.0))).max_h(height.min(px(420.0))))
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
                    _ => {}
                }
                cx.stop_propagation();
            }))
            .children(heading)
            .child(body)
            .focus_trap("dictionary-focus", &self.focus);
        // Keep the selected word visible while containing pointer input, so a
        // dismissal cannot turn a page or begin another text selection.
        let movement_target = cx.entity().downgrade();
        let scrim = components::absolute_full_size()
            .id("dictionary-scrim")
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this._dismiss_task = Task::ready(());
                    this.element_click = WordClick::default();
                    if !this.selection_only && this.anchor.is_some_and(|bounds| bounds.contains(&event.position)) {
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
                if !this.selection_only && event.mouse_position().is_some() && event.modifiers() == Default::default() && this.anchor.is_some_and(|bounds| bounds.contains(&event.position())) {
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
        let overlay = if let Some((bounds, above, _)) = placement {
            // Anchor origins snap to device pixels. Reserve one pixel so the
            // minimum gap survives rounding with fractional panel heights.
            let gap = px(8.0 + 1.0 / window.scale_factor());
            let panel = anchored()
                .anchor(if above { Anchor::BottomLeft } else { Anchor::TopLeft })
                .position(point(bounds.left(), if above { bounds.top() - gap } else { bounds.bottom() + gap }))
                .snap_to_window_with_margin(Edges { top: insets.top + px(12.0), right: insets.right + px(12.0), bottom: insets.bottom + px(12.0), left: insets.left + px(12.0) })
                .child(panel);
            components::anchor_to_window(components::absolute_full_size().child(scrim).child(panel), window)
        } else {
            components::centered_modal(scrim, panel, window)
        };
        gpui::deferred(overlay).with_priority(10)
    }
}

#[cfg(test)]
#[path = "dictionary_tests.rs"]
mod tests;
