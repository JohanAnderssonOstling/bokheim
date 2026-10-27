use gpui::prelude::*;
use gpui::{ElementId, SharedString, px};
use crate::Button;
use gpui_component::{Disableable, Icon, IconName, Sizable};

use crate::{BrowserTheme, SPACE_MD, TEXT_SM, topbar_action_height_rem};

/// Common entry point for interactive controls.
///
/// The application button component owns hover styling; this shared entry
/// point also gives every control the same pointer behavior.
pub fn base_button(id: impl Into<ElementId>) -> Button {
    base_button_impl(id.into())
}

fn base_button_impl(id: ElementId) -> Button {
    Button::new(id).cursor_pointer()
}

/// Square, with a light border distinct from the background.
///
/// `.secondary()` is deliberately absent: it filled the button with
/// `colors.secondary`, which the theme sets to the same value as the page
/// background, so the fill asserted a surface that was never visible. The
/// border is the whole control, and it sits on the quiet tier — `border` is
/// where hover travels to, not where the button rests.
pub(crate) fn outlined_button(id: impl Into<ElementId>, theme: BrowserTheme) -> Button {
    outlined_button_impl(id.into(), theme)
}

fn outlined_button_impl(id: ElementId, theme: BrowserTheme) -> Button {
    base_button_impl(id).outline().border_color(theme.rule)
}

/// A borderless square icon action, `topbar_action_height_rem()` on a side (2rem,
/// or the touch target on phones), with a transparent background and
/// a 1rem icon and normal text — the standard size and
/// shape shared by every square icon button in the app (square corners, to
/// match every other native surface and control). Semantic wrappers own any
/// contextual state colour (hover, selection) on top of this baseline.
pub fn outlined_icon_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: impl Into<Icon>, theme: BrowserTheme) -> Button {
    outlined_icon_button_impl(id.into(), label.into(), icon.into(), theme)
}

fn outlined_icon_button_impl(id: ElementId, label: SharedString, icon: Icon, _theme: BrowserTheme) -> Button {
    base_button_impl(id).ghost().icon(icon).size(gpui::rems(topbar_action_height_rem())).tooltip(label)
}

/// A compact neutral outlined text button.
///
/// Consumers may add an icon, selected state, or contextual geometry without
/// duplicating the native secondary-outline treatment.
pub fn outlined_text_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    outlined_text_button_impl(id.into(), label.into())
}

fn outlined_text_button_impl(id: ElementId, label: SharedString) -> Button {
    base_button_impl(id).label(label).with_size(gpui_component::Size::Small).secondary().outline()
}

/// Deleting something for good: the standard icon button wearing `danger`, an
/// icon where the actions beside it that keep things (Restore, Save) are words.
pub fn delete_icon_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    delete_icon_button_impl(id.into(), label.into(), theme)
}

fn delete_icon_button_impl(id: ElementId, label: SharedString, theme: BrowserTheme) -> Button {
    outlined_icon_button_impl(id, label, IconName::Delete.into(), theme).text_color(theme.danger)
}

/// The way out of a dialog, at the size of the primary action it stands beside.
///
/// [`outlined_text_button`] is deliberately compact — it is a control inside
/// chrome, sized against toolbars — and a dialog's Cancel is not that button:
/// paired with a default-size primary, it reads as the lesser of two controls
/// that should be equals.
pub fn dialog_cancel_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    dialog_cancel_button_impl(id.into(), label.into())
}

fn dialog_cancel_button_impl(id: ElementId, label: SharedString) -> Button {
    let tooltip = crate::shortcut_tooltip(label.clone(), "escape");
    base_button_impl(id).label(label).tooltip(tooltip).secondary().outline()
}

pub fn primary_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: impl Into<Icon>, disabled: bool) -> Button {
    primary_action_button_impl(id.into(), label.into(), icon.into(), disabled)
}

fn primary_action_button_impl(id: ElementId, label: SharedString, icon: Icon, disabled: bool) -> Button {
    base_button_impl(id).label(label).icon(icon).primary().h(gpui::rems(topbar_action_height_rem())).px(px(SPACE_MD)).disabled(disabled)
}

pub fn secondary_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    secondary_action_button_impl(id.into(), label.into(), icon, disabled)
}

fn secondary_action_button_impl(id: ElementId, label: SharedString, icon: IconName, disabled: bool) -> Button {
    base_button_impl(id).label(label).icon(icon).secondary().outline().h(gpui::rems(topbar_action_height_rem())).px(px(SPACE_MD)).disabled(disabled)
}

/// A text action that carries no frame of its own, for the way out of a form
/// rather than the way through it. It has no icon: an icon would give it the
/// weight the shape exists to avoid.
pub fn link_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme, disabled: bool) -> Button {
    link_button_impl(id.into(), label.into(), theme, disabled)
}

fn link_button_impl(id: ElementId, label: SharedString, _theme: BrowserTheme, disabled: bool) -> Button {
    base_button_impl(id).label(label).ghost().with_size(gpui_component::Size::Small).px(px(0.0)).text_size(gpui::rems(TEXT_SM)).disabled(disabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Hsla, Render, TestAppContext, Window, canvas, div, rgb};
    use std::{cell::Cell, rc::Rc};

    struct DisabledButtonTest {
        disabled_ink: Rc<Cell<Option<Hsla>>>,
        enabled_ink: Rc<Cell<Option<Hsla>>>,
        clicks: Rc<Cell<usize>>,
    }

    impl Render for DisabledButtonTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().children([true, false].map(|disabled| {
                let ink = if disabled { self.disabled_ink.clone() } else { self.enabled_ink.clone() };
                let clicks = self.clicks.clone();
                let id = if disabled { "disabled-custom-button" } else { "enabled-custom-button" };
                base_button(id)
                    .ghost()
                    .size(px(40.0))
                    .text_color(rgb(0xcc0000))
                    .bg(rgb(0x00cc00))
                    .disabled(disabled)
                    .debug_selector(move || id.to_owned())
                    .child(canvas(move |_, window, _| ink.set(Some(window.text_style().color)), |_, _, _, _| {}).size(px(16.0)))
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
            }))
        }
    }

    #[gpui::test]
    fn disabled_buttons_override_custom_ink_and_do_not_activate(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let disabled_ink = Rc::new(Cell::new(None));
        let enabled_ink = Rc::new(Cell::new(None));
        let clicks = Rc::new(Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let disabled_ink = disabled_ink.clone();
            let enabled_ink = enabled_ink.clone();
            let clicks = clicks.clone();
            move |_, _| DisabledButtonTest { disabled_ink, enabled_ink, clicks }
        });
        cx.run_until_parked();
        let disabled_color = cx.update(|window, cx| {
            let _ = window.draw(cx);
            crate::browser_theme(cx).text_muted.opacity(0.5)
        });
        assert_eq!(disabled_ink.get(), Some(disabled_color), "caller colors must not override the disabled foreground");
        assert_eq!(enabled_ink.get(), Some(rgb(0xcc0000).into()), "enabled controls keep their custom colors");
        let disabled = cx.debug_bounds("disabled-custom-button").unwrap();
        cx.simulate_click(disabled.center(), Default::default());
        cx.run_until_parked();
        assert_eq!(clicks.get(), 0);
        let enabled = cx.debug_bounds("enabled-custom-button").unwrap();
        cx.simulate_click(enabled.center(), Default::default());
        cx.run_until_parked();
        assert_eq!(clicks.get(), 1);
    }
}
