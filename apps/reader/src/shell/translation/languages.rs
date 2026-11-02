//! Filterable language choices using the shared list and touch popover.
use book_model::normalize_search_text;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, Task, Window, div, px, rems};
use gpui_component::{
    Icon, IconName, IndexPath,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
    popover::Popover,
};
use ui_components as components;

use super::TranslationPopup;

fn language_label(code: &str) -> String {
    if let Some((_, name)) = LANGUAGES.iter().find(|(key, _)| *key == code) {
        return (*name).into();
    }
    if let Some((base, variant)) = code.split_once('-') {
        if let Some((_, name)) = LANGUAGES.iter().find(|(key, _)| *key == base) {
            return format!("{name} ({})", variant.to_ascii_uppercase());
        }
    }
    code.into()
}

pub(super) fn picker(source: bool, current: Option<&str>, window: &mut Window, cx: &mut Context<TranslationPopup>) -> AnyElement {
    let label = current.map(language_label).unwrap_or_else(|| "Choose language".into());
    let current = current.map(str::to_owned);
    let owner = cx.entity().downgrade();
    let id = if source { "translation-source" } else { "translation-target" };
    let list_state = window.use_keyed_state((id, 0usize), cx, |_, _| None::<Entity<ListState<LanguageList>>>);
    let closing_state = list_state.clone();
    Popover::new(id)
        .anchor(gpui::Anchor::BottomLeft)
        .appearance(false)
        .flex_1()
        .min_w_0()
        .trigger(components::outlined_text_button((id, 1usize), label).no_fill().w_full().min_w_0().child(Icon::new(IconName::ChevronDown)))
        .on_open_change(move |open, _, cx| {
            if !open {
                closing_state.update(cx, |state, _| *state = None);
            }
        })
        .content(move |_, window, cx| {
            let list = match list_state.read(cx).clone() {
                Some(list) => list,
                None => {
                    let list = cx.new(|cx| {
                        let choices = LanguageList::new(current.clone());
                        let selected = choices.choices.iter().position(|(code, _)| Some(code) == current.as_ref()).unwrap_or(0);
                        let mut list = ListState::new(choices, window, cx).searchable(true);
                        list.set_selected_index(Some(IndexPath::new(selected)), window, cx);
                        list
                    });
                    list.update(cx, |list, cx| list.focus(window, cx));
                    let popover = cx.entity();
                    let owner = owner.clone();
                    window
                        .subscribe(&list, cx, move |list, event: &ListEvent, window, cx| {
                            match event {
                                ListEvent::Confirm(index) => {
                                    if let Some(code) = list.read(cx).delegate().code(*index).map(str::to_owned) {
                                        let _ = owner.update(cx, |owner, cx| owner.select_language(source, code, cx));
                                    }
                                }
                                ListEvent::Cancel => {}
                                _ => return,
                            }
                            popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                        })
                        .detach();
                    list_state.update(cx, |state, _| *state = Some(list.clone()));
                    list
                }
            };
            let theme = components::browser_theme(cx);
            let viewport = window.viewport_size();
            let insets = window.insets().effective();
            components::modal_form_surface(theme)
                .w(px(280.0))
                .max_w((viewport.width - insets.left - insets.right - px(24.0)).max(px(1.0)))
                .h(px(320.0).min((viewport.height - insets.top - insets.bottom - px(24.0)).max(px(1.0))))
                .p_0()
                .gap_0()
                .child(List::new(&list).search_placeholder("Filter languages"))
        })
        .into_any_element()
}

struct LanguageList {
    choices: Vec<(String, String)>,
    filtered: Vec<usize>,
    current: Option<String>,
}

impl LanguageList {
    fn new(current: Option<String>) -> Self {
        let mut choices = LANGUAGES.iter().map(|(code, label)| ((*code).to_owned(), (*label).to_owned())).collect::<Vec<_>>();
        if let Some(code) = current.as_deref().filter(|code| !LANGUAGES.iter().any(|(key, _)| key == code)) {
            choices.push((code.into(), language_label(code)));
        }
        Self { filtered: (0..choices.len()).collect(), choices, current }
    }

    fn code(&self, index: IndexPath) -> Option<&str> {
        self.filtered.get(index.row).map(|index| self.choices[*index].0.as_str())
    }
}

impl ListDelegate for LanguageList {
    type Item = ListItem;

    fn perform_search(&mut self, query: &str, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Task<()> {
        let query = normalize_search_text(query);
        self.filtered = self.choices.iter().enumerate().filter(|(_, (_, label))| normalize_search_text(label).contains(&query)).map(|(index, _)| index).collect();
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.filtered.len()
    }

    fn render_item(&mut self, index: IndexPath, _: &mut Window, _: &mut Context<ListState<Self>>) -> Option<Self::Item> {
        let (code, label) = &self.choices[*self.filtered.get(index.row)?];
        Some(ListItem::new(code.clone()).h(rems(components::settings_control_size_rem())).check_icon(IconName::Check).confirmed(self.current.as_ref() == Some(code)).child(label.clone()))
    }

    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}

    fn render_empty(&mut self, _: &mut Window, _: &mut Context<ListState<Self>>) -> impl IntoElement {
        div().p(px(16.0)).child("No languages found.")
    }
}

const LANGUAGES: &[(&str, &str)] = &[
    ("af", "Afrikaans"),
    ("sq", "Albanian"),
    ("ar", "Arabic"),
    ("hy", "Armenian"),
    ("bn", "Bengali"),
    ("bg", "Bulgarian"),
    ("ca", "Catalan"),
    ("zh", "Chinese"),
    ("zh-cn", "Chinese (Simplified)"),
    ("zh-tw", "Chinese (Traditional)"),
    ("hr", "Croatian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("et", "Estonian"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("ka", "Georgian"),
    ("de", "German"),
    ("el", "Greek"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hu", "Hungarian"),
    ("is", "Icelandic"),
    ("id", "Indonesian"),
    ("ga", "Irish"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("la", "Latin"),
    ("lv", "Latvian"),
    ("lt", "Lithuanian"),
    ("ms", "Malay"),
    ("nb", "Norwegian Bokmål"),
    ("nn", "Norwegian Nynorsk"),
    ("fa", "Persian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("pt-br", "Portuguese (Brazil)"),
    ("pt-pt", "Portuguese (Portugal)"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sr", "Serbian"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("es", "Spanish"),
    ("sw", "Swahili"),
    ("sv", "Swedish"),
    ("tl", "Tagalog"),
    ("ta", "Tamil"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("ur", "Urdu"),
    ("vi", "Vietnamese"),
    ("cy", "Welsh"),
];
