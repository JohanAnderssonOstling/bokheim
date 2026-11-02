//! Translation content inside the shared selection popup.
use std::collections::BTreeMap;

use app::{
    AppClient,
    translation::{MAX_SELECTION_BYTES, SELECTION_TOO_LONG, language_code},
};
use content_address::BookId;
use gpui::prelude::*;
use gpui::{ClipboardItem, Context, EventEmitter, IntoElement, Render, Task, Window, div, px};
use gpui_component::{Disableable, IconName, popover::Popover};
use ui_components as components;

use super::dictionary::Closed;
use crate::settings::ReaderSettings;

mod languages;

pub(super) struct TranslationPopup {
    backend: AppClient,
    book_id: BookId,
    text: String,
    source: Option<String>,
    target: String,
    result: Option<Result<String, String>>,
    generation: u64,
    cache: BTreeMap<(String, String), String>,
    _request: Task<()>,
}

impl EventEmitter<Closed> for TranslationPopup {}

impl TranslationPopup {
    pub(super) fn new(backend: AppClient, book_id: BookId, text: String, languages: &[String], cx: &mut Context<Self>) -> Self {
        let preferences = ReaderSettings::preferences(cx);
        let target = preferences.translation_language.as_deref().and_then(language_code).or_else(|| sys_locale::get_locales().find_map(|tag| language_code(&tag))).unwrap_or_else(|| "en".into());
        let source = preferences.translation_sources.get(&book_id).and_then(|code| language_code(code)).or_else(|| languages.iter().find_map(|tag| language_code(tag)));
        let mut popup = Self { backend, book_id, text, source, target, result: None, generation: 0, cache: BTreeMap::new(), _request: Task::ready(()) };
        popup.translate(cx);
        popup
    }

    fn translate(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self._request = Task::ready(());
        self.result = None;
        let error = if self.text.len() > MAX_SELECTION_BYTES {
            Some(SELECTION_TOO_LONG)
        } else if self.text.trim().is_empty() {
            Some("Select some text to translate.")
        } else {
            None
        };
        if let Some(error) = error {
            self.result = Some(Err(error.into()));
            cx.notify();
            return;
        }
        let Some(source) = self.source.clone() else {
            self.result = Some(Err("Choose the source language to translate this selection.".into()));
            cx.notify();
            return;
        };
        let target = self.target.clone();
        if source == target {
            self.result = Some(Ok(self.text.clone()));
        } else if let Some(cached) = self.cache.get(&(source.clone(), target.clone())) {
            self.result = Some(Ok(cached.clone()));
        } else {
            let backend = self.backend.clone();
            let text = self.text.clone();
            let generation = self.generation;
            self._request = cx.spawn(async move |this, cx| {
                let result = backend.translate_selection(text, source.clone(), target.clone()).await;
                let _ = this.update(cx, |this, cx| {
                    if this.generation != generation {
                        return;
                    }
                    if let Ok(text) = &result {
                        // Keep a few choices so revisiting a language is local.
                        if this.cache.len() >= 8 {
                            this.cache.pop_first();
                        }
                        this.cache.insert((source, target), text.clone());
                    }
                    this.result = Some(result);
                    cx.notify();
                });
            });
        }
        cx.notify();
    }

    fn select_language(&mut self, source: bool, code: String, cx: &mut Context<Self>) {
        if source {
            if self.source.as_deref() == Some(code.as_str()) {
                return;
            }
            self.source = Some(code.clone());
            ReaderSettings::update(cx, |preferences| {
                preferences.translation_sources.insert(self.book_id, code);
            });
        } else {
            if self.target == code {
                return;
            }
            self.target = code.clone();
            ReaderSettings::update(cx, |preferences| preferences.translation_language = Some(code));
        }
        self.translate(cx);
    }
}

impl Render for TranslationPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let translated = self.result.as_ref().and_then(|result| result.as_ref().ok()).cloned();
        let can_copy = translated.is_some();
        let info = Popover::new("translation-info").anchor(gpui::Anchor::TopRight).appearance(false).trigger(components::outlined_icon_button("translation-info-button", "Translation source and privacy", IconName::Info).no_fill()).child(
            components::modal_form_surface(theme)
                .id("translation-info-content")
                .w(px(280.0))
                .max_w((window.viewport_size().width - px(48.0)).max(px(1.0)))
                .max_h((window.viewport_size().height - px(48.0)).max(px(1.0)))
                .overflow_y_scroll()
                .gap(px(8.0))
                .child(components::section_title("MyMemory"))
                .child("Selected text and the language pair are sent directly to MyMemory. It receives your IP address and says submitted segments are stored long-term.")
                .when(self.source.is_some(), |panel| panel.child(div().flex().items_center().gap(px(8.0)).child("Translate from").child(languages::picker(true, self.source.as_deref(), window, cx))))
                .child(components::outlined_text_button("translation-privacy", "MyMemory privacy policy").ghost().no_fill().on_click(|_, _, cx| cx.open_url("https://mymemory.translated.net/terms-and-conditions"))),
        );
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .flex_none()
            .child(div().flex().items_center().gap(px(8.0)).flex_1().min_w_0().child("Translate to").child(languages::picker(false, Some(&self.target), window, cx)))
            .child(components::outlined_icon_button("translation-copy", "Copy translation", IconName::Copy).no_fill().disabled(!can_copy).on_click(move |_, _, cx| {
                if let Some(text) = &translated {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                }
            }))
            .child(info)
            .child(components::outlined_icon_button("translation-close", "Close translation", IconName::Close).no_fill().on_click(cx.listener(|_, _, _, cx| cx.emit(Closed))));
        let text = match &self.result {
            None => "Translating…".to_owned(),
            Some(Ok(text)) | Some(Err(text)) => text.clone(),
        };
        div()
            .w_full()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(header)
            .when(self.source.is_none() && self.text.len() <= MAX_SELECTION_BYTES, |panel| {
                panel.child(div().flex().items_center().gap(px(8.0)).flex_none().child("Translate from").child(languages::picker(true, self.source.as_deref(), window, cx)))
            })
            .child(div().id("translation-result").min_h_0().overflow_y_scroll().child(text))
    }
}
