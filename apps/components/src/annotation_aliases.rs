//! Inline aliases for a single recurring note. Input drafts belong to its ID.
use crate::{AnnotationEditorTarget, RADIUS_SM, SPACE_SM, SPACE_XS, TEXT_XS, base_button, column, link_button, reader_state_note};
use book_model::Annotation;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, Render, SharedString, Window, div, px, rems};
use gpui_component::input::{Enter, Escape, Input, InputState};
use ui_text::text;

enum AliasChange {
    Add(String),
    Remove(String),
}

type AliasHandler = Box<dyn Fn(AliasChange, &mut App)>;

struct AliasEditor {
    annotation: Annotation,
    input: Entity<InputState>,
    adding: bool,
    error: Option<&'static str>,
    change: AliasHandler,
}

pub fn annotation_alias_controls<C: AnnotationEditorTarget>(annotation: &Annotation, target: &Entity<C>, window: &mut Window, cx: &mut Context<C>) -> AnyElement {
    let parent = target.downgrade();
    let id = annotation.id.clone();
    let key = SharedString::from(format!("annotation-aliases-{:?}-{id}", target.entity_id()));
    let editor = window.use_keyed_state(key, cx, |window, cx| AliasEditor {
        annotation: annotation.clone(),
        input: cx.new(|cx| InputState::new(window, cx).placeholder(text!(reader::alias_name_or_phrase))),
        adding: false,
        error: None,
        change: Box::new(move |change, cx| {
            let _ = parent.update(cx, |this, cx| match change {
                AliasChange::Add(alias) => this.add_annotation_alias(id.clone(), alias, cx),
                AliasChange::Remove(alias) => this.remove_annotation_alias(id.clone(), alias, cx),
            });
        }),
    });
    editor.update(cx, |editor, cx| {
        if editor.annotation != *annotation {
            editor.annotation = annotation.clone();
            cx.notify();
        }
    });
    editor.into_any_element()
}

impl AliasEditor {
    fn show_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.adding = true;
        self.error = None;
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let alias = self.input.read(cx).value().split_whitespace().collect::<Vec<_>>().join(" ");
        self.error = if alias.is_empty() || alias.contains('\0') {
            Some(text!(reader::alias_enter_name_or_phrase))
        } else if self.annotation.recurring_phrases().any(|phrase| phrase.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase() == alias.to_lowercase()) {
            Some(text!(reader::alias_already_matches))
        } else {
            let mut aliases = self.annotation.anchor.recurring_aliases().to_vec();
            aliases.push(alias.clone());
            self.annotation.anchor = self.annotation.anchor.with_recurring_aliases(aliases);
            (self.change)(AliasChange::Add(alias), cx);
            self.input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
            None
        };
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }
}

impl Render for AliasEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::browser_theme(cx);
        let aliases = self.annotation.anchor.recurring_aliases();
        let mut section = column(SPACE_XS).w_full().min_w_0();
        if !aliases.is_empty() || self.adding {
            section = section.child(reader_state_note(text!(reader::also_match), theme));
        }
        let mut chips = div().w_full().min_w_0().flex().flex_wrap().items_center().gap(px(SPACE_XS));
        for (index, alias) in aliases.iter().enumerate() {
            let remove_alias = alias.clone();
            let label = format!(text!(reader::remove_alias), alias);
            let remove = base_button(("annotation-alias-remove", index)).ghost().label("×").tooltip(label.clone()).aria_label(label).w(px(22.0)).h(px(22.0)).p(px(0.0)).on_click(cx.listener(move |this, _, _, cx| {
                (this.change)(AliasChange::Remove(remove_alias.clone()), cx);
                this.error = None;
                cx.notify();
            }));
            chips = chips.child(
                div()
                    .max_w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(SPACE_XS))
                    .pl(px(SPACE_SM))
                    .pr(px(2.0))
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(theme.rule)
                    .text_size(rems(TEXT_XS))
                    .child(div().min_w_0().truncate().child(alias.clone()))
                    .child(remove),
            );
        }
        let label = if aliases.is_empty() && !self.adding { text!(reader::add_aliases) } else { text!(reader::add_alias) };
        chips = chips.child(link_button("annotation-alias-add", label, false).text_color(theme.text_accent).on_click(cx.listener(|this, _, window, cx| this.show_input(window, cx))));
        section = section.child(chips);
        if self.adding {
            section = section.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(SPACE_XS))
                    .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                        this.add(window, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &Escape, _, cx| {
                        this.adding = false;
                        this.error = None;
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.input)))
                    .child(crate::outlined_text_button("annotation-alias-submit", text!(browse::add)).on_click(cx.listener(|this, _, window, cx| this.add(window, cx)))),
            );
        }
        section.when_some(self.error, |section, error| section.child(reader_state_note(error, theme)))
    }
}
