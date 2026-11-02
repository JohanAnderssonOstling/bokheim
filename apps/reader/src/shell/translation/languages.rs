//! Filterable language choices using the shared list and touch popover.
use ui_text::text;

use book_model::normalize_search_text;
use language_tags::LanguageTag;
use reader_lookups::parse_language_tag;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, Task, Window, div, px, rems};
use gpui_component::{
    Icon, IconName, IndexPath,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
    popover::Popover,
};
use ui_components as components;

use super::TranslationPopup;

fn language_label(tag: &LanguageTag) -> String {
    if let Some((_, name)) = LANGUAGES.iter().find(|(key, _)| key.eq_ignore_ascii_case(tag.as_str())) {
        return (*name).into();
    }
    if let Some((_, name)) = LANGUAGES.iter().find(|(key, _)| *key == tag.primary_language()) {
        let variant = tag.extended_language_subtags().chain(tag.script()).chain(tag.region()).chain(tag.variant_subtags()).chain(tag.extension()).chain(tag.private_use()).collect::<Vec<_>>().join("-");
        return format!(text!(reader::language_variant), variant.to_ascii_uppercase(), name = name);
    }
    tag.to_string()
}

pub(super) fn picker(source: bool, current: Option<&LanguageTag>, window: &mut Window, cx: &mut Context<TranslationPopup>) -> AnyElement {
    let label = current.map(language_label).unwrap_or_else(|| text!(reader::choose_language).into());
    let current = current.cloned();
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
                                    if let Some(code) = list.read(cx).delegate().code(*index).cloned() {
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
                .child(List::new(&list).search_placeholder(text!(reader::filter_languages)))
        })
        .into_any_element()
}

struct LanguageList {
    choices: Vec<(LanguageTag, String)>,
    filtered: Vec<usize>,
    current: Option<LanguageTag>,
}

impl LanguageList {
    fn new(current: Option<LanguageTag>) -> Self {
        let mut choices = LANGUAGES.iter().filter_map(|(code, label)| Some((parse_language_tag(code)?, (*label).to_owned()))).collect::<Vec<_>>();
        if let Some(code) = current.as_ref().filter(|code| !choices.iter().any(|(key, _)| key == *code)) {
            choices.push((code.clone(), language_label(code)));
        }
        Self { filtered: (0..choices.len()).collect(), choices, current }
    }

    fn code(&self, index: IndexPath) -> Option<&LanguageTag> {
        self.filtered.get(index.row).map(|index| &self.choices[*index].0)
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
        Some(ListItem::new(code.to_string()).h(rems(components::settings_control_size_rem())).check_icon(IconName::Check).confirmed(self.current.as_ref() == Some(code)).child(label.clone()))
    }

    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}

    fn render_empty(&mut self, _: &mut Window, _: &mut Context<ListState<Self>>) -> impl IntoElement {
        div().p(px(16.0)).child(text!(reader::no_languages_found))
    }
}

const LANGUAGES: &[(&str, &str)] = &[
    ("af", text!(reader::afrikaans)),
    ("sq", text!(reader::albanian)),
    ("ar", text!(common::arabic)),
    ("hy", text!(reader::armenian)),
    ("bn", text!(reader::bengali)),
    ("bg", text!(common::bulgarian)),
    ("ca", text!(reader::catalan)),
    ("zh", text!(common::chinese)),
    ("zh-cn", text!(reader::chinese_simplified)),
    ("zh-tw", text!(reader::chinese_traditional)),
    ("hr", text!(reader::croatian)),
    ("cs", text!(reader::czech)),
    ("da", text!(common::danish)),
    ("nl", text!(common::dutch)),
    ("en", text!(common::english)),
    ("et", text!(reader::estonian)),
    ("fi", text!(common::finnish)),
    ("fr", text!(common::french)),
    ("ka", text!(reader::georgian)),
    ("de", text!(common::german)),
    ("el", text!(common::greek)),
    ("he", text!(common::hebrew)),
    ("hi", text!(common::hindi)),
    ("hu", text!(reader::hungarian)),
    ("is", text!(common::icelandic)),
    ("id", text!(reader::indonesian)),
    ("ga", text!(reader::irish)),
    ("it", text!(common::italian)),
    ("ja", text!(common::japanese)),
    ("ko", text!(reader::korean)),
    ("la", text!(common::latin)),
    ("lv", text!(reader::latvian)),
    ("lt", text!(reader::lithuanian)),
    ("ms", text!(reader::malay)),
    ("nb", text!(common::norwegian_bokmal)),
    ("nn", text!(common::norwegian_nynorsk)),
    ("fa", text!(reader::persian)),
    ("pl", text!(common::polish)),
    ("pt", text!(common::portuguese)),
    ("pt-br", text!(reader::portuguese_brazil)),
    ("pt-pt", text!(reader::portuguese_portugal)),
    ("ro", text!(reader::romanian)),
    ("ru", text!(common::russian)),
    ("sr", text!(reader::serbian)),
    ("sk", text!(reader::slovak)),
    ("sl", text!(reader::slovenian)),
    ("es", text!(common::spanish)),
    ("sw", text!(reader::swahili)),
    ("sv", text!(common::swedish)),
    ("tl", text!(common::tagalog)),
    ("ta", text!(common::tamil)),
    ("th", text!(reader::thai)),
    ("tr", text!(reader::turkish)),
    ("uk", text!(common::ukrainian)),
    ("ur", text!(reader::urdu)),
    ("vi", text!(reader::vietnamese)),
    ("cy", text!(reader::welsh)),
];
