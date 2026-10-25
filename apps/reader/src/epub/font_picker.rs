use gpui::prelude::*;
use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, RenderOnce, Window, div, px, rems};
use gpui_component::Selectable;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::popover::Popover;
use ui_components::{self as components, ScrollableElement};

use crate::settings::ReaderSettings;

pub(super) struct FontSelected(pub String);

pub(super) struct FontPicker {
    input: Entity<InputState>,
    families: Vec<String>,
    open: bool,
    previous_focus: Option<FocusHandle>,
}

impl EventEmitter<FontSelected> for FontPicker {}

impl FontPicker {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search fonts"));
        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let query = this.input.read(cx).value().to_lowercase();
                if let Some(family) = this.families.iter().find(|family| family.to_lowercase().contains(&query)).cloned() {
                    this.choose(family, window, cx);
                }
            }
            cx.notify();
        })
        .detach();
        let mut families = cx.try_global::<components::SystemFontFamilies>().map_or_else(|| cx.text_system().all_font_names(), |fonts| fonts.0.clone());
        families.retain(|family| !family.starts_with('.'));
        families.sort_by_cached_key(|family| family.to_lowercase());
        families.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        Self { input, families, open: false, previous_focus: None }
    }

    pub(super) fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.previous_focus = window.focused(cx);
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn choose(&mut self, family: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        self.input.update(cx, |input, cx| input.set_value("", window, cx));
        cx.emit(FontSelected(family));
        cx.notify();
    }
}

impl Render for FontPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let selected = ReaderSettings::preferences(cx).system_font_family.clone();
        let query = self.input.read(cx).value().to_lowercase();
        let mut rows = div().id("font-list").max_h(px((f32::from(window.viewport_size().height) * 0.45).min(320.0))).overflow_y_scrollbar().flex().flex_col().gap(px(components::SPACE_XXS));
        let mut matches = 0;
        for (index, family) in self.families.iter().enumerate().filter(|(_, family)| family.to_lowercase().contains(&query)) {
            matches += 1;
            let family = family.clone();
            rows = rows.child(
                components::reader_state_card(("system-font", index), selected.as_ref() == Some(&family), theme)
                    .flex_none()
                    .cursor_pointer()
                    .min_h(px(44.0))
                    .child(div().min_w_0().text_ellipsis().child(family.clone()))
                    .child(div().font_family(family.clone()).text_size(rems(1.15)).child("Aa Bb Cc 123"))
                    .on_click(cx.listener(move |this, _, window, cx| this.choose(family.clone(), window, cx))),
            );
        }
        if matches == 0 {
            rows = rows.child(div().p(px(components::SPACE_SM)).text_color(theme.text_muted).child("No matching fonts"));
        }
        let panel = div()
            .key_context("ReaderFontPicker")
            .w(px((f32::from(window.viewport_size().width) - 32.0).clamp(1.0, 300.0)))
            .flex()
            .flex_col()
            .gap(px(components::SPACE_SM))
            .child(div().text_color(theme.text).child("Custom font"))
            .when(selected.as_ref().is_some_and(|family| !self.families.contains(family)), |panel| panel.child(div().text_color(theme.text_muted).child("Saved font unavailable on this device")))
            .child(Input::new(&self.input))
            .child(rows);
        Popover::new("reader-system-fonts")
            .anchor(gpui::Anchor::TopRight)
            .track_focus(&self.input.read(cx).focus_handle(cx))
            .open(self.open)
            .on_open_change(cx.listener(|this, open, window, cx| {
                this.open = *open;
                if !*open && let Some(focus) = this.previous_focus.take() {
                    focus.focus(window, cx);
                }
                cx.notify();
            }))
            .trigger(FontTile { family: selected, selected: false })
            .child(panel)
    }
}

/// The popover trigger uses the same picture tile as the three preset faces.
#[derive(IntoElement)]
struct FontTile {
    family: Option<String>,
    selected: bool,
}

impl Selectable for FontTile {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.family.is_some() || self.selected
    }
}

impl RenderOnce for FontTile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let sample = components::reader_typeface_sample(None).when_some(self.family.clone(), |sample, family| sample.font_family(family));
        components::reader_picture_tile("reader-system-font-trigger", sample, self.family.unwrap_or_else(|| "Custom".into()), self.selected, components::browser_theme(cx)).w_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::settings::ReaderPreferences;
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    fn searching_and_selecting_a_font_closes_the_picker(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            components::register_fonts(cx).unwrap();
            cx.set_global(ReaderSettings { preferences: ReaderPreferences::default(), save: Rc::new(|_, _, _| {}) });
            cx.set_global(components::SystemFontFamilies(vec!["Raleway".into(), "NV Garamond".into()]));
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let picker = cx.new(|cx| FontPicker::new(window, cx));
            gpui_component::Root::new(picker, window, cx)
        });
        let picker = cx.update(|_, cx| root.read(cx).view().clone().downcast::<FontPicker>().ok().unwrap());
        let selected = Rc::new(RefCell::new(Vec::new()));
        let target = selected.clone();
        let _subscription = cx.update(|window, cx| {
            picker.update(cx, |picker, cx| picker.show(window, cx));
            cx.subscribe(&picker, move |_, event: &FontSelected, _| target.borrow_mut().push(event.0.clone()))
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("g a r enter");
        cx.run_until_parked();
        assert_eq!(*selected.borrow(), vec!["NV Garamond"]);
        picker.read_with(cx, |picker, cx| {
            assert!(!picker.open);
            assert!(picker.input.read(cx).value().is_empty());
        });
        cx.update(|window, cx| picker.update(cx, |picker, cx| picker.show(window, cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        picker.read_with(cx, |picker, _| assert!(!picker.open));
        assert_eq!(selected.borrow().len(), 1);
    }
}
