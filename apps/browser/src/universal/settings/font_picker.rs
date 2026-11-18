//! Interface-font preference controls around the shared font picker.
use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, FocusHandle, Render, Window, div};
use ui_components as components;
use ui_text::text;

pub(super) struct FontSelected(pub String);

pub(super) struct FontPicker {
    picker: Entity<components::FontPicker>,
    selected: Option<String>,
    active: bool,
    trigger_focus: FocusHandle,
}

impl EventEmitter<FontSelected> for FontPicker {}

impl FontPicker {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker = cx.new(|cx| components::FontPicker::new(window, cx));
        cx.subscribe_in(&picker, window, |this, _, event, window, cx| match event {
            components::FontPickerEvent::Selected(family) => this.choose(family.clone(), window, cx),
            components::FontPickerEvent::Dismissed => {
                this.trigger_focus.focus(window, cx);
                cx.notify();
            }
        })
        .detach();
        Self { picker, selected: None, active: false, trigger_focus: cx.focus_handle().tab_stop(true) }
    }

    pub(super) fn set_selected(&mut self, family: Option<String>, cx: &mut Context<Self>) {
        self.active = family.is_some();
        self.selected = family.clone();
        self.picker.update(cx, |picker, cx| picker.set_selected(family, cx));
    }

    pub(super) fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub(super) fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active
            && let Some(family) = self.selected.clone()
        {
            self.choose(family, window, cx);
        }
        self.picker.update(cx, |picker, cx| picker.show(self.selected.clone(), |_, _| true, window, cx));
        cx.notify();
    }

    fn choose(&mut self, family: String, _: &mut Window, cx: &mut Context<Self>) {
        self.active = true;
        self.selected = Some(family.clone());
        cx.emit(FontSelected(family));
        cx.notify();
    }
}

impl Render for FontPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let preview_family = self.selected.clone().filter(|family| !cfg!(target_os = "android") || cx.try_global::<components::SystemFontFamilies>().is_some_and(|fonts| fonts.0.contains(family)));
        let sample = components::reader_typeface_sample(None).when_some(preview_family, |sample, family| sample.font_family(family));
        let tooltip = self.selected.clone().unwrap_or_else(|| text!(settings::choose_an_installed_font).into());
        let label = self.selected.clone().unwrap_or_else(|| text!(common::custom).into());
        let trigger = components::reader_picture_tile("ui-font-custom", sample, label, self.picker.read(cx).is_open() || self.active, theme)
            .w_full()
            .track_focus(&self.trigger_focus)
            .debug_selector(|| "ui-font-custom-trigger".into())
            .tooltip(tooltip)
            .on_click(cx.listener(|this, _, window, cx| this.show(window, cx)));
        div().w_full().child(trigger).child(self.picker.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, point, px};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[gpui::test]
    fn interface_font_picker_is_lazy_scrolls_and_applies_the_actual_family(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            components::register_fonts(cx).unwrap();
            let families = (0..1000).map(|index| format!("Font {index:04}")).chain(["NV Garamond".into()]).collect();
            cx.set_global(components::SystemFontFamilies(families));
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let picker = cx.new(|cx| FontPicker::new(window, cx));
            gpui_component::Root::new(picker, window, cx)
        });
        let picker = cx.update(|_, cx| root.read(cx).view().clone().downcast::<FontPicker>().ok().unwrap());
        picker.read_with(cx, |picker, cx| {
            assert!(!picker.picker.read(cx).catalog().loaded);
            assert!(picker.picker.read(cx).catalog().families.is_empty());
        });
        let selected = Rc::new(RefCell::new(None));
        let target = selected.clone();
        let _subscription = cx.update(|_, cx| cx.subscribe(&picker, move |_, event: &FontSelected, _| *target.borrow_mut() = Some(event.0.clone())));
        cx.run_until_parked();
        let bounds = cx.debug_bounds("ui-font-custom-trigger").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
        let desktop = cx.debug_bounds("font-picker-panel").unwrap();
        cx.simulate_resize(gpui::size(px(390.0), px(640.0)));
        cx.run_until_parked();
        let phone = cx.debug_bounds("font-picker-panel").unwrap();
        assert!(phone.size.width < desktop.size.width);
        assert!(phone.size.height < desktop.size.height);
        assert!(phone.left() >= px(0.0) && phone.right() <= px(390.0));
        assert!(phone.top() >= px(0.0) && phone.bottom() <= px(640.0));
        assert!(cx.debug_bounds("font-picker-row-0").is_some());
        assert!(cx.debug_bounds("font-picker-row-999").is_none());
        let bounds = cx.debug_bounds("font-picker-list").unwrap();
        cx.simulate_event(ScrollWheelEvent { position: bounds.center(), delta: ScrollDelta::Pixels(point(px(0.0), px(-132.0))), ..Default::default() });
        cx.run_until_parked();
        picker.read_with(cx, |picker, cx| assert!(picker.picker.read(cx).scroll().0.borrow().base_handle.offset().y < px(0.0)));
        cx.simulate_keystrokes("g a r enter");
        cx.run_until_parked();
        assert_eq!(*selected.borrow(), Some("NV Garamond".into()));
        picker.read_with(cx, |picker, cx| {
            assert!(picker.picker.read(cx).is_open());
            assert_eq!(picker.selected.as_deref(), Some("NV Garamond"));
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        picker.read_with(cx, |picker, cx| assert!(!picker.picker.read(cx).is_open()));
        let bounds = cx.debug_bounds("ui-font-custom-trigger").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
        picker.read_with(cx, |picker, cx| {
            assert!(picker.picker.read(cx).is_open());
            assert!(picker.picker.read(cx).query(cx).is_empty());
            assert_eq!(picker.picker.read(cx).catalog().filtered.len(), 1001);
        });
        cx.simulate_keystrokes("escape");
        picker.update(cx, |picker, cx| {
            picker.set_active(false);
            cx.notify();
        });
        *selected.borrow_mut() = None;
        cx.run_until_parked();
        let bounds = cx.debug_bounds("ui-font-custom-trigger").unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
        assert_eq!(*selected.borrow(), Some("NV Garamond".into()), "clicking the saved font restores it immediately");
        picker.read_with(cx, |picker, cx| {
            assert!(picker.active);
            assert!(picker.picker.read(cx).is_open());
            assert_eq!(picker.selected.as_deref(), Some("NV Garamond"));
        });
    }
}
