//! Filterable language choices using the shared list and touch popover.
use ui_text::text;
use components::LANGUAGE_NAMES as LANGUAGES;

use book_model::normalize_search_text;
use language_tags::LanguageTag;
use reader_lookups::parse_language_tag;
use gpui::prelude::*;
use gpui::{AnyElement, App, Bounds, Context, Entity, IntoElement, Pixels, Task, Window, div, px, rems};
use gpui_component::{
    Icon, IconName, IndexPath,
    list::{ListDelegate, ListEvent, ListItem, ListState},
    popover::Popover,
};
use ui_components as components;

use super::TranslationPopup;
use super::super::dictionary::{desktop_picker, mobile_picker};

pub(super) fn language_label(tag: &LanguageTag) -> String {
    if let Some((_, name)) = LANGUAGES.iter().find(|(key, _)| key.eq_ignore_ascii_case(tag.as_str())) {
        return (*name).into();
    }
    if let Some((_, name)) = LANGUAGES.iter().find(|(key, _)| *key == tag.primary_language()) {
        let variant = tag.extended_language_subtags().chain(tag.script()).chain(tag.region()).chain(tag.variant_subtags()).chain(tag.extension()).chain(tag.private_use()).collect::<Vec<_>>().join("-");
        return format!(text!(reader::language_variant), variant.to_ascii_uppercase(), name = name);
    }
    tag.to_string()
}

/// Combine locale preferences and the active keyboard's recognized language.
pub(super) fn system_languages(cx: &App) -> Vec<LanguageTag> {
    let mut languages = Vec::new();
    for locale in sys_locale::get_locales().filter_map(|locale| parse_language_tag(&locale)) {
        let primary = locale.primary_language();
        let regional = locale.region().map(|region| format!("{primary}-{region}"));
        let code = if primary == "zh" && locale.script() == Some("Hant") {
            "zh-tw"
        } else if primary == "zh" && locale.script() == Some("Hans") {
            "zh-cn"
        } else {
            regional.as_deref().filter(|regional| LANGUAGES.iter().any(|(code, _)| code.eq_ignore_ascii_case(regional))).unwrap_or(primary)
        };
        if let Some(language) = parse_language_tag(code).filter(|language| !languages.contains(language)) {
            languages.push(language);
        }
    }
    // Layout IDs are not language tags: XKB's "se" means Swedish, while
    // BCP 47's "se" means Northern Sami. Match the reported language name.
    let layout = cx.keyboard_layout();
    let keyboard = layout.name().split('(').next().unwrap_or_default().trim();
    if let Some(language) = LANGUAGES.iter().find(|(_, name)| name.eq_ignore_ascii_case(keyboard)).and_then(|(code, _)| parse_language_tag(code)).filter(|language| !languages.contains(language)) {
        languages.push(language);
    }
    if languages.is_empty() {
        languages.push(LanguageTag::parse("en").expect("fixed language tag"));
    }
    languages
}

pub(super) fn picker(source: bool, current: Option<&LanguageTag>, bounds: Option<Bounds<Pixels>>, window: &mut Window, cx: &mut Context<TranslationPopup>) -> AnyElement {
    let label = current.map_or_else(|| text!(reader::choose_language).to_owned(), language_label);
    let owner = cx.entity().downgrade();
    let id = if source { "translation-source" } else { "translation-target" };
    let open_bounds = window.use_keyed_state((id, 1usize), cx, |_, _| None::<Bounds<Pixels>>);
    let bounds = (*open_bounds.read(cx)).or(bounds).map(|mut bounds| {
        let safe = window.insets().effective();
        let top = safe.top + px(12.0);
        let bottom = window.viewport_size().height - safe.bottom - px(12.0);
        bounds.size.height = px(400.0).min((bottom - top).max(px(1.0)));
        bounds.origin.y = bounds.origin.y.clamp(top, (bottom - bounds.size.height).max(top));
        bounds
    });
    let list_state = window.use_keyed_state((id, 0usize), cx, |_, _| None::<Entity<ListState<LanguageList>>>);
    let closing_state = list_state.clone();
    let menu_owner = owner.clone();
    Popover::new(id)
        .anchor(gpui::Anchor::BottomLeft)
        .when(components::touch_ui(), |popover| popover.anchor(gpui::Anchor::TopLeft).position(gpui::point(px(0.0), px(0.0))).overlay_closable(false))
        .when_some(bounds.filter(|_| !components::touch_ui()), |popover, bounds| popover.anchor(gpui::Anchor::TopLeft).position(bounds.origin))
        .appearance(false)
        .flex_1()
        .min_w_0()
        .trigger(components::outlined_text_button((id, 1usize), label).no_fill().w_full().min_w_0().child(Icon::new(IconName::ChevronDown)))
        .on_open_change(move |open, _, cx| {
            open_bounds.update(cx, |saved, _| *saved = if *open { bounds } else { None });
            let _ = menu_owner.update(cx, |owner, cx| {
                owner.language_picker_open = *open;
                cx.notify();
            });
            if !open {
                closing_state.update(cx, |state, _| *state = None);
            }
        })
        .content(move |_, window, cx| {
            let list = match list_state.read(cx).clone() {
                Some(list) => list,
                None => {
                    let list = cx.new(|cx| ListState::new(LanguageList::new(cx), window, cx).searchable(true));
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
            if components::touch_ui() {
                return mobile_picker(&list, text!(reader::filter_languages), window, cx);
            }
            desktop_picker(&list, text!(reader::filter_languages), bounds, window, cx)
        })
        .into_any_element()
}

struct LanguageList {
    choices: Vec<(LanguageTag, String)>,
    filtered: Vec<usize>,
}

impl LanguageList {
    fn new(cx: &App) -> Self {
        let system = system_languages(cx);
        let mut choices = LANGUAGES.iter().filter_map(|(code, label)| Some((parse_language_tag(code)?, (*label).to_owned()))).collect::<Vec<_>>();
        for code in &system {
            if !choices.iter().any(|(key, _)| key == code) {
                choices.push((code.clone(), language_label(code)));
            }
        }
        choices.sort_by_key(|(code, _)| system.iter().position(|language| language == code).unwrap_or(system.len()));
        Self { filtered: (0..choices.len()).collect(), choices }
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

    fn render_item(&mut self, index: IndexPath, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Option<Self::Item> {
        let (code, label) = &self.choices[*self.filtered.get(index.row)?];
        Some(
            ListItem::new(code.to_string())
                .selection_highlight(false)
                .h(rems(components::settings_control_size_rem()))
                .when(components::touch_ui(), |row| row.h(px(48.0)).px(px(components::SPACE_MD)))
                .border_b_1()
                .border_color(components::browser_theme(cx).rule)
                .child(label.clone()),
        )
    }

    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}

    fn render_empty(&mut self, _: &mut Window, _: &mut Context<ListState<Self>>) -> impl IntoElement {
        div().p(px(16.0)).child(text!(reader::no_languages_found))
    }
}
