use ui_text::text;

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, FocusHandle, IntoElement, KeyDownEvent, Render, TextSystem, Window, div, px, rems};
use html_view_gpui::is_private_font_family;
use ui_components as components;

use crate::settings::{ReaderFontRole, ReaderSettings};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FontSelected {
    pub role: ReaderFontRole,
    pub family: String,
}

pub(super) struct FontPicker {
    picker: Entity<components::FontPicker>,
    open: bool,
    active_role: Option<ReaderFontRole>,
    role_focus: [FocusHandle; 4],
    cjk: bool,
}

impl EventEmitter<FontSelected> for FontPicker {}

fn book_font_filter(_: &TextSystem, family: &str) -> bool {
    !is_private_font_family(family) && family != components::CJK_FONT_FAMILY
}

fn font_supports_cjk(text_system: &TextSystem, family: &str) -> bool {
    if family.starts_with('.') || is_private_font_family(family) || family == components::CJK_FONT_FAMILY {
        return false;
    }
    text_system.font_supports_any_text(&gpui::font(family.to_owned()), &["漢字中文", "汉字中文", "あいうえ", "アイウエ", "한글한국"])
}

impl FontPicker {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker = cx.new(|cx| components::FontPicker::new(window, cx));
        cx.subscribe_in(&picker, window, |this, _, event, window, cx| match event {
            components::FontPickerEvent::Selected(family) => this.choose(family.clone(), window, cx),
            components::FontPickerEvent::Dismissed => this.back(window, cx),
        })
        .detach();
        Self { picker, open: false, active_role: None, cjk: false, role_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)) }
    }

    pub(super) fn set_cjk(&mut self, cjk: bool) {
        self.cjk = cjk;
    }

    pub(super) fn cjk_focus(&self) -> &FocusHandle {
        &self.role_focus[ReaderFontRole::Cjk.index()]
    }

    pub(super) fn activate_cjk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(family) = ReaderSettings::preferences(cx).saved_cjk_font_family().map(str::to_owned) {
            self.hide(cx);
            cx.emit(FontSelected { role: ReaderFontRole::Cjk, family });
        } else {
            self.show_cjk(window, cx);
        }
    }

    pub(super) fn show_cjk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.select_role(ReaderFontRole::Cjk, window, cx);
    }

    pub(super) fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.active_role = None;
        self.picker.update(cx, |picker, cx| picker.hide(cx));
        self.role_focus[0].focus(window, cx);
        cx.notify();
    }

    fn select_role(&mut self, role: ReaderFontRole, window: &mut Window, cx: &mut Context<Self>) {
        self.active_role = Some(role);
        let selected = ReaderSettings::preferences(cx).custom_font_family(role).map(str::to_owned);
        let filter = if self.cjk { font_supports_cjk } else { book_font_filter };
        self.picker.update(cx, |picker, cx| picker.show(selected, filter, window, cx));
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(role) = self.active_role.take() {
            self.role_focus[role.index()].focus(window, cx);
        }
        cx.notify();
    }

    fn choose(&mut self, family: String, _: &mut Window, cx: &mut Context<Self>) {
        let Some(role) = self.active_role else { return };
        cx.emit(FontSelected { role, family });
        cx.notify();
    }
}

impl Render for FontPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let preferences = ReaderSettings::preferences(cx).clone();
        let choices = ReaderFontRole::ALL.into_iter().map(|role| {
            let family = preferences.custom_font_family(role).map(str::to_owned);
            let name = family.clone().unwrap_or_else(|| text!(reader::book).into());
            let active = self.active_role == Some(role);
            components::reader_state_card(("custom-font-role", role.index()), active, theme)
                .flex_1()
                .w_auto()
                .border_1()
                .border_color(if active { theme.accent } else { theme.rule })
                .rounded(px(components::RADIUS_SM))
                .px(px(components::SPACE_XS))
                .py(px(components::SPACE_SM))
                .items_center()
                .justify_center()
                .text_color(if active { theme.text } else { theme.text_muted })
                .cursor_pointer()
                .child(div().text_size(rems(components::TEXT_XS)).child(role.label()))
                .debug_selector(move || format!("custom-font-role-{}", role.index()))
                .track_focus(&self.role_focus[role.index()])
                .child(div().w_full().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_center().text_size(rems(components::TEXT_XS)).when_some(family, |label, family| label.font_family(family)).child(name))
                .on_click(cx.listener(move |this, _, window, cx| this.select_role(role, window, cx)))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.select_role(role, window, cx);
                        cx.stop_propagation();
                    }
                }))
        });
        // The Typeface container already supplies SPACE_XXS between its children.
        let rows = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(components::SPACE_SM))
            .when(!self.cjk && (self.open || preferences.custom_fonts_active()), |rows| rows.mt(px(components::SPACE_SM - components::SPACE_XXS)).child(components::reader_tile_group("reader-custom-font-roles").children(choices)));
        if let Some(role) = self.active_role {
            let selected = preferences.custom_font_family(role).map(str::to_owned);
            self.picker.update(cx, |picker, cx| picker.set_selected(selected, cx));
        }
        rows.child(self.picker.clone())
    }
}

impl FontPicker {
    pub(super) fn hide(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.active_role = None;
        self.picker.update(cx, |picker, cx| picker.hide(cx));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::settings::{CjkFontChoice, ReaderPreferences};
    use gpui::{AppContext, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, point};

    #[gpui::test]
    fn saved_cjk_font_switches_directly_and_editing_keeps_book_until_a_choice(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            components::register_fonts(cx).unwrap();
            let preferences = ReaderPreferences { system_cjk_font_family: Some("Saved CJK".into()), system_font_family: Some("Latin font".into()), custom_fonts_enabled: true, ..ReaderPreferences::default() };
            cx.set_global(ReaderSettings { preferences, save: Rc::new(|_, _, _| {}) });
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let picker = cx.new(|cx| {
                let mut picker = FontPicker::new(window, cx);
                picker.set_cjk(true);
                picker
            });
            gpui_component::Root::new(picker, window, cx)
        });
        let picker = cx.update(|_, cx| root.read(cx).view().clone().downcast::<FontPicker>().ok().unwrap());
        let _subscription = cx.update(|window, cx| {
            let subscription = cx.subscribe(&picker, |_, event: &FontSelected, cx| {
                ReaderSettings::update(cx, |preferences| preferences.set_custom_font_family(event.role, event.family.clone()));
            });
            picker.update(cx, |picker, cx| picker.activate_cjk(window, cx));
            subscription
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("font-picker-panel").is_none());
        picker.read_with(cx, |picker, cx| {
            assert!(!picker.picker.read(cx).catalog().loaded, "restoring a saved choice must not enumerate fonts");
            assert_eq!(ReaderSettings::preferences(cx).cjk_font_choice, CjkFontChoice::Custom);
        });

        cx.update(|window, cx| {
            ReaderSettings::update(cx, |preferences| preferences.cjk_font_choice = CjkFontChoice::Publisher);
            picker.update(cx, |picker, cx| {
                picker.picker.update(cx, |picker, cx| picker.install_font_catalog(vec!["Saved CJK".into(), "Replacement CJK".into()], cx));
                picker.show_cjk(window, cx);
            });
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("font-picker-panel").is_some());
        picker.read_with(cx, |_, cx| assert_eq!(ReaderSettings::preferences(cx).cjk_font_choice, CjkFontChoice::Publisher));
        cx.simulate_keystrokes("r e p enter");
        cx.run_until_parked();
        picker.read_with(cx, |_, cx| {
            let preferences = ReaderSettings::preferences(cx);
            assert_eq!(preferences.saved_cjk_font_family(), Some("Replacement CJK"));
            assert_eq!(preferences.cjk_font_choice, CjkFontChoice::Custom);
            assert_eq!(preferences.system_font_family.as_deref(), Some("Latin font"));
            assert!(preferences.custom_fonts_active());
        });
        cx.update(|window, cx| {
            ReaderSettings::update(cx, |preferences| preferences.cjk_font_choice = CjkFontChoice::Publisher);
            picker.update(cx, |picker, cx| picker.activate_cjk(window, cx));
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("font-picker-panel").is_none());
        picker.read_with(cx, |_, cx| assert_eq!(ReaderSettings::preferences(cx).cjk_font_choice, CjkFontChoice::Custom));

        cx.update(|window, cx| {
            ReaderSettings::update(cx, |preferences| {
                preferences.system_cjk_font_family = None;
                preferences.cjk_font_choice = CjkFontChoice::Publisher;
            });
            picker.update(cx, |picker, cx| picker.activate_cjk(window, cx));
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("font-picker-panel").is_some());
        picker.read_with(cx, |_, cx| assert_eq!(ReaderSettings::preferences(cx).cjk_font_choice, CjkFontChoice::Publisher));
    }

    #[gpui::test]
    fn cjk_picker_restores_local_choice_and_keeps_latin_roles_separate(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            components::register_fonts(cx).unwrap();
            let preferences = ReaderPreferences { cjk_font_choice: CjkFontChoice::Custom, system_cjk_font_family: Some("Unavailable CJK font".into()), ..ReaderPreferences::default() };
            let preferences = serde_json::from_slice(&serde_json::to_vec(&preferences).unwrap()).unwrap();
            cx.set_global(ReaderSettings { preferences, save: Rc::new(|_, _, _| {}) });
            cx.set_global(components::SystemFontFamilies(vec!["System CJK".into()]));
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let picker = cx.new(|cx| {
                let mut picker = FontPicker::new(window, cx);
                picker.set_cjk(true);
                picker
            });
            gpui_component::Root::new(picker, window, cx)
        });
        let picker = cx.update(|_, cx| root.read(cx).view().clone().downcast::<FontPicker>().ok().unwrap());
        picker.read_with(cx, |picker, cx| assert!(!picker.picker.read(cx).catalog().loaded));
        let _subscription = cx.update(|window, cx| {
            let subscription = cx.subscribe(&picker, |_, event: &FontSelected, cx| {
                ReaderSettings::update(cx, |preferences| preferences.set_custom_font_family(event.role, event.family.clone()));
            });
            picker.update(cx, |picker, cx| {
                picker.picker.update(cx, |picker, cx| picker.install_font_catalog(vec!["System CJK".into()], cx));
                picker.show_cjk(window, cx)
            });
            subscription
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("custom-font-role-0").is_none());
        assert!(cx.debug_bounds("font-picker-panel").is_some());
        assert!(cx.debug_bounds("font-picker-unavailable").is_some());
        picker.read_with(cx, |_, cx| {
            assert_eq!(ReaderSettings::preferences(cx).system_cjk_font_family.as_deref(), Some("Unavailable CJK font"));
        });
        cx.simulate_keystrokes("c j k enter");
        cx.run_until_parked();
        assert!(cx.debug_bounds("font-picker-unavailable").is_none());
        picker.read_with(cx, |picker, cx| {
            assert_eq!(picker.active_role, Some(ReaderFontRole::Cjk));
            let preferences = ReaderSettings::preferences(cx);
            assert_eq!(preferences.cjk_font_choice, CjkFontChoice::Custom);
            assert_eq!(preferences.system_cjk_font_family.as_deref(), Some("System CJK"));
            assert!(!preferences.custom_fonts_active());
        });
    }

    #[gpui::test]
    fn large_font_catalog_is_lazy_virtualized_and_scrolls_with_the_wheel(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            components::register_fonts(cx).unwrap();
            cx.set_global(ReaderSettings { preferences: ReaderPreferences::default(), save: Rc::new(|_, _, _| {}) });
            cx.set_global(components::SystemFontFamilies((0..1000).map(|index| format!("Font {index:04}")).collect()));
        });
        let (root, cx) = cx.add_window_view(|window, cx| {
            let picker = cx.new(|cx| FontPicker::new(window, cx));
            gpui_component::Root::new(picker, window, cx)
        });
        let picker = cx.update(|_, cx| root.read(cx).view().clone().downcast::<FontPicker>().ok().unwrap());
        picker.read_with(cx, |picker, cx| {
            assert!(!picker.picker.read(cx).catalog().loaded);
            assert!(picker.picker.read(cx).catalog().families.is_empty(), "reader startup must not enumerate fonts");
        });
        cx.update(|window, cx| picker.update(cx, |picker, cx| picker.show(window, cx)));
        cx.run_until_parked();
        assert!(cx.debug_bounds("custom-font-role-0").is_some());
        picker.read_with(cx, |picker, cx| assert!(!picker.picker.read(cx).catalog().loaded, "opening the three role choices must not enumerate fonts"));
        let role = cx.debug_bounds("custom-font-role-0").unwrap();
        cx.simulate_click(role.center(), Modifiers::default());
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
        assert!(cx.debug_bounds("font-picker-row-999").is_none(), "offscreen families must not be shaped");
        let bounds = cx.debug_bounds("font-picker-list").unwrap();
        cx.simulate_event(ScrollWheelEvent { position: bounds.center(), delta: ScrollDelta::Pixels(point(px(0.0), px(-132.0))), ..Default::default() });
        cx.run_until_parked();
        picker.read_with(cx, |picker, cx| assert!(picker.picker.read(cx).scroll().0.borrow().base_handle.offset().y < px(0.0)));
        cx.simulate_keystrokes("f o n t space 0 9 9 9");
        cx.run_until_parked();
        picker.read_with(cx, |picker, cx| {
            assert_eq!(picker.picker.read(cx).catalog().filtered.len(), 1);
            assert_eq!(picker.picker.read(cx).catalog().families[picker.picker.read(cx).catalog().filtered[0]], "Font 0999");
            assert_eq!(picker.picker.read(cx).scroll().0.borrow().base_handle.offset().y, px(0.0));
        });
    }

    #[gpui::test]
    fn each_role_can_choose_any_font_without_closing_the_picker(cx: &mut TestAppContext) {
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
            cx.subscribe(&picker, move |_, event: &FontSelected, cx| {
                ReaderSettings::update(cx, |preferences| preferences.set_custom_font_family(event.role, event.family.clone()));
                target.borrow_mut().push(event.clone());
            })
        });
        cx.run_until_parked();
        for role in ReaderFontRole::ALL {
            cx.update(|window, cx| picker.update(cx, |picker, cx| picker.select_role(role, window, cx)));
            cx.run_until_parked();
            cx.simulate_keystrokes("g a r enter");
            cx.run_until_parked();
            assert!(cx.debug_bounds("font-picker-panel").is_some());
            picker.read_with(cx, |picker, cx| {
                assert!(picker.open);
                assert_eq!(picker.active_role, Some(role));
                assert_eq!(ReaderSettings::preferences(cx).custom_font_family(role), Some("NV Garamond"));
            });
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
        }
        assert_eq!(*selected.borrow(), ReaderFontRole::ALL.map(|role| FontSelected { role, family: "NV Garamond".into() }));
        cx.update(|window, cx| picker.update(cx, |picker, cx| picker.select_role(ReaderFontRole::Serif, window, cx)));
        picker.read_with(cx, |picker, cx| {
            assert!(picker.open);
            assert!(picker.picker.read(cx).query(cx).is_empty());
            assert_eq!(picker.picker.read(cx).catalog().filtered.len(), picker.picker.read(cx).catalog().families.len(), "reopening must restore the unfiltered catalog");
        });
        cx.update(|window, cx| picker.update(cx, |picker, cx| picker.show(window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| picker.update(cx, |picker, cx| picker.select_role(ReaderFontRole::Serif, window, cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        picker.read_with(cx, |picker, cx| {
            assert!(picker.open);
            assert_eq!(picker.active_role, None);
        });
        assert_eq!(selected.borrow().len(), 3);
    }
}
