//! Translation content inside the shared selection popup.
use ui_text::text;

use std::collections::{HashMap, HashSet};

use content_address::BookId;
use gpui::prelude::*;
use gpui::{ClipboardItem, Context, EventEmitter, IntoElement, Render, Task, Window, div, px, rems};
use gpui_component::{IconName, Theme, popover::Popover, text::TextView};
use language_tags::LanguageTag;
use reader_lookups::{
    Client as LookupClient, parse_language_tag,
    translation::{TranslationError, MAX_SELECTION_BYTES, SELECTION_TOO_LONG},
};
use ui_components as components;

use super::dictionary::lookup_popover;
use crate::settings::ReaderSettings;

mod languages;

pub(super) struct TranslationPopup {
    pub(super) picker_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    lookups: LookupClient,
    book_id: BookId,
    text: String,
    source: Option<LanguageTag>,
    targets: Vec<LanguageTag>,
    translations: Vec<Translation>,
    error: Option<String>,
    language_picker_open: bool,
    cache: HashMap<(LanguageTag, LanguageTag), String>,
}

struct Translation {
    language: LanguageTag,
    result: Option<Result<String, TranslationError>>,
    _request: Task<()>,
}

pub(super) struct LanguagesChanged {
    pub source: Option<LanguageTag>,
    pub targets: Vec<LanguageTag>,
}

pub(super) struct ResultsChanged;

impl EventEmitter<LanguagesChanged> for TranslationPopup {}
impl EventEmitter<ResultsChanged> for TranslationPopup {}

impl TranslationPopup {
    pub(super) fn new(book_id: BookId, text: String, languages: &[String], cx: &mut Context<Self>) -> Self {
        let lookups = super::lookups::client(cx);
        let preferences = ReaderSettings::preferences(cx);
        let mut targets = preferences.translation_languages.clone().unwrap_or_default();
        let mut seen = HashSet::new();
        targets.retain(|tag| seen.insert(tag.clone()));
        let source = preferences.translation_sources.get(&book_id).cloned().or_else(|| languages.iter().find_map(|tag| parse_language_tag(tag)));
        if targets.is_empty() {
            targets = languages::system_languages(cx);
            ReaderSettings::update(cx, |preferences| preferences.translation_languages = Some(targets.clone()));
        }
        let mut popup = Self { picker_bounds: None, lookups, book_id, text, source, targets, translations: Vec::new(), error: None, language_picker_open: false, cache: HashMap::new() };
        popup.translate(cx);
        popup
    }

    fn translate(&mut self, cx: &mut Context<Self>) {
        self.error = if self.text.len() > MAX_SELECTION_BYTES {
            Some(SELECTION_TOO_LONG.into())
        } else if self.text.trim().is_empty() {
            Some(text!(reader::select_some_text_to_translate).into())
        } else if self.source.is_none() {
            Some(text!(reader::choose_the_source_language_to_translate_this_selection).into())
        } else {
            None
        };
        self.translations.retain(|translation| self.targets.contains(&translation.language));
        if self.error.is_some() {
            self.translations.clear();
            cx.notify();
            return;
        }
        let Some(source) = self.source.clone() else { return };
        let mut ready_result = false;
        for target in &self.targets {
            if self.translations.iter().any(|translation| &translation.language == target) {
                continue;
            }
            let result = if &source == target { Some(Ok(self.text.clone())) } else { self.cache.get(&(source.clone(), target.clone())).cloned().map(Ok) };
            ready_result |= result.is_some();
            let request = if result.is_some() {
                Task::ready(())
            } else {
                let lookups = self.lookups.clone();
                let text = self.text.clone();
                let source = source.clone();
                let target = target.clone();
                cx.spawn(async move |this, cx| {
                    let result = lookups.translate_selection(text, source.clone(), target.clone()).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.source.as_ref() != Some(&source) {
                            return;
                        }
                        let Some(translation) = this.translations.iter_mut().find(|translation| translation.language == target) else { return };
                        if let Ok(text) = &result {
                            // Keep active targets and a few previous choices local.
                            if this.cache.len() >= this.targets.len().max(8) {
                                if let Some(key) = this.cache.keys().find(|(cached_source, cached_target)| cached_source != &source || !this.targets.contains(cached_target)).cloned() {
                                    this.cache.remove(&key);
                                }
                            }
                            this.cache.insert((source, target), text.clone());
                        }
                        translation.result = Some(result);
                        cx.emit(ResultsChanged);
                        cx.notify();
                    });
                })
            };
            self.translations.push(Translation { language: target.clone(), result, _request: request });
        }
        self.translations.sort_by_key(|translation| self.targets.iter().position(|target| target == &translation.language));
        if ready_result {
            cx.emit(ResultsChanged);
        }
        cx.notify();
    }

    fn select_language(&mut self, source: bool, code: LanguageTag, cx: &mut Context<Self>) {
        if source {
            if self.source.as_ref() == Some(&code) {
                return;
            }
            self.source = Some(code.clone());
            self.translations.clear();
            ReaderSettings::update(cx, |preferences| {
                preferences.translation_sources.insert(self.book_id, code);
            });
        } else {
            if let Some(index) = self.targets.iter().position(|target| target == &code) {
                if self.targets.len() == 1 {
                    return;
                }
                self.targets.remove(index);
            } else {
                self.targets.push(code);
            }
            ReaderSettings::update(cx, |preferences| preferences.translation_languages = Some(self.targets.clone()));
        }
        self.translate(cx);
        cx.emit(self.languages());
    }

    pub(super) fn languages(&self) -> LanguagesChanged {
        LanguagesChanged { source: self.source.clone(), targets: self.targets.clone() }
    }

    pub(super) fn is_loading(&self) -> bool {
        self.translations.iter().any(|translation| translation.result.is_none())
    }

    fn retry(&mut self, language: &LanguageTag, cx: &mut Context<Self>) {
        self.translations.retain(|translation| &translation.language != language);
        self.translate(cx);
    }

}

impl TranslationPopup {
    pub(super) fn header_actions(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let info = Popover::new("translation-info")
            .overlay_closable(!self.language_picker_open)
            .anchor(gpui::Anchor::TopRight)
            .appearance(false)
            .trigger(components::outlined_icon_button("translation-info-button", text!(reader::translation_source_and_privacy), IconName::Info).no_fill())
            .child(lookup_popover(
                components::modal_form_surface(theme)
                    .font_family(Theme::global(cx).font_family.clone())
                    .text_size(Theme::global(cx).font_size)
                    .id("translation-info-content")
                    .w(rems(17.5))
                    .max_w((window.viewport_size().width - px(48.0)).max(px(1.0)))
                    .max_h((window.viewport_size().height - px(48.0)).max(px(1.0)))
                    .overflow_y_scroll()
                    .gap(px(8.0))
                    .child(components::section_title(text!(reader::mymemory)))
                    .child(text!(reader::selected_text_and_the_language_pair_are_sent_directly_to_mymemory_it))
                    .when(self.source.is_some(), |panel| panel.child(div().flex().items_center().gap(px(8.0)).child(text!(reader::translate_from)).child(languages::picker(true, self.source.as_slice(), self.picker_bounds, window, cx))))
                    .child(components::outlined_text_button("translation-privacy", text!(reader::mymemory_privacy_policy)).ghost().no_fill().on_click(|_, _, cx| cx.open_url("https://mymemory.translated.net/terms-and-conditions"))),
            ));
        div().flex_none().flex().items_center().gap(px(8.0)).child(info)
            .children(self.translations.iter().filter_map(|translation| {
                let translated = translation.result.as_ref()?.as_ref().ok()?.clone();
                let language = &translation.language;
                let label = if self.targets.len() > 1 {
                    format!("{} ({})", text!(reader::copy_translation), languages::language_label(language))
                } else {
                    text!(reader::copy_translation).to_owned()
                };
                Some(components::outlined_icon_button(format!("translation-copy-{language}"), label, IconName::Copy).no_fill().on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(translated.clone()));
                }))
            })).into_any_element()
    }
}

impl Render for TranslationPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let header =
            div().flex().items_center().gap(px(8.0)).flex_none().child(div().flex().items_center().gap(px(8.0)).flex_1().min_w_0().child(text!(reader::translate_to)).child(languages::picker(false, &self.targets, self.picker_bounds, window, cx)));
        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(header)
            .when(self.source.is_none() && self.text.len() <= MAX_SELECTION_BYTES, |panel| {
                panel.child(div().flex().items_center().gap(px(8.0)).flex_none().child(text!(reader::translate_from)).child(languages::picker(true, self.source.as_slice(), self.picker_bounds, window, cx)))
            })
            .child(div().id("translation-results").flex().flex_col().gap(px(8.0)).children(self.error.clone()).children(self.translations.iter().map(|translation| {
                let language = translation.language.clone();
                let result = match &translation.result {
                    None => text!(reader::translating).to_owned(),
                    Some(Ok(text)) => text.clone(),
                    Some(Err(error)) => translation_error_message(error).to_owned(),
                };
                div()
                    .min_w_0()
                    .flex_none()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .when(self.targets.len() > 1, |row| row.child(div().flex_none().max_w(px(100.0)).text_sm().text_color(theme.text_muted).child(languages::language_label(&language))))
                    .child(div().flex_1().min_w_0().child(TextView::plain(format!("translation-text-{language}"), result).selectable(matches!(&translation.result, Some(Ok(_))))))
                    .when(matches!(&translation.result, Some(Err(_))), |panel| {
                        panel.child(components::outlined_text_button(format!("translation-retry-{language}"), text!(common::retry)).on_click(cx.listener(move |this, _, _, cx| this.retry(&language, cx))))
                    })
            })))
    }
}

fn translation_error_message(error: &TranslationError) -> &'static str {
    let fallback = match error {
        TranslationError::TooLong => SELECTION_TOO_LONG,
        TranslationError::Empty => text!(reader::select_some_text_to_translate),
        TranslationError::Request(_) => text!(reader::translation_failed),
        TranslationError::RateLimited => return text!(reader::translation_limit_reached),
        TranslationError::DailyLimit => return text!(reader::translation_daily_limit_reached),
        TranslationError::UnsupportedLanguages => text!(reader::translation_languages_unsupported),
        TranslationError::EmptyResponse => text!(reader::translation_empty),
    };
    components::user_facing_error(error, fallback)
}
