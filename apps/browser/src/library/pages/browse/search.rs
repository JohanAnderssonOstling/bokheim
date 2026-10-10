//! The browse page's search box: whether it is open, what has been typed, and
//! when a pause in typing is long enough to search. The page hears one event,
//! carrying the query to run.

use std::time::Duration;

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, Subscription, Task, Window};
use gpui_component::input::{InputEvent, InputState};
use ui_components as components;

/// The query to search for, reported once typing has settled.
pub(super) struct SearchChanged(pub(super) String);

pub(super) struct BrowseSearch {
    /// Created the first time the box opens, since an input needs a window.
    input: Option<Entity<InputState>>,
    open: bool,
    /// What was last reported, so an edit that changes nothing reports nothing.
    query: String,
    debounce: Option<Task<()>>,
    _input_changes: Option<Subscription>,
}

impl EventEmitter<SearchChanged> for BrowseSearch {}

impl BrowseSearch {
    pub(super) fn new() -> Self {
        Self { input: None, open: false, query: String::new(), debounce: None, _input_changes: None }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.as_ref().is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
    }

    pub(super) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = match &self.input {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search books or authors"));
                self._input_changes = Some(cx.subscribe(&input, |search, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        search.input_changed(cx);
                    }
                }));
                self.input = Some(input.clone());
                input
            }
        };
        self.open = true;
        input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Hides the box. What was typed stays, and so does the search it ran.
    pub(super) fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    /// Empties the box without reporting it, for a page that has already
    /// dropped the query itself. The change the input reports matches the
    /// emptied query, so nothing is searched.
    pub(super) fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.debounce = None;
        if let Some(input) = &self.input {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    pub(super) fn render(&self) -> Option<gpui::Div> {
        self.open.then(|| self.input.as_ref().map(components::browser_search_control)).flatten()
    }

    /// Reports the typed query once typing pauses.
    fn input_changed(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.input else { return };
        let query = input.read(cx).value().to_string();
        if query == self.query {
            return;
        }
        self.query = query.clone();
        let timer = cx.background_executor().timer(Duration::from_millis(250));
        self.debounce = Some(cx.spawn(async move |search, cx| {
            timer.await;
            let _ = search.update(cx, |search, cx| {
                search.debounce = None;
                cx.emit(SearchChanged(query));
            });
        }));
    }
}
