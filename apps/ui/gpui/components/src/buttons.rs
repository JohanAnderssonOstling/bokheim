use gpui::prelude::*;
use gpui::{ElementId, Length, SharedString, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Icon, IconName, Sizable};

use crate::{BrowserTheme, SPACE_MD, TEXT_SM, TOPBAR_ACTION_HEIGHT};

pub(crate) fn outlined_button(id: impl Into<ElementId>, theme: BrowserTheme) -> Button {
    Button::new(id).secondary().outline().border_color(theme.border)
}

/// An outlined square icon action. Semantic wrappers own their dimensions and
/// any contextual colour or background treatment.
pub fn outlined_icon_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: impl Into<Icon>, size: impl Into<Length> + Clone, radius: f32, theme: BrowserTheme) -> Button {
    outlined_button(id, theme).icon(icon).size(size).rounded(px(radius)).tooltip(label)
}

/// A compact neutral outlined text button.
///
/// Consumers may add an icon, selected state, or contextual geometry without
/// duplicating the native secondary-outline treatment.
pub fn outlined_text_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label).with_size(gpui_component::Size::Small).secondary().outline()
}

pub fn primary_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    Button::new(id).label(label).icon(icon).primary().h(px(TOPBAR_ACTION_HEIGHT)).px(px(SPACE_MD)).disabled(disabled)
}

pub fn secondary_action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, disabled: bool) -> Button {
    Button::new(id).label(label).icon(icon).secondary().outline().h(px(TOPBAR_ACTION_HEIGHT)).px(px(SPACE_MD)).disabled(disabled)
}

/// A text action that carries no frame of its own, for the way out of a form
/// rather than the way through it. It has no icon: an icon would give it the
/// weight the shape exists to avoid.
pub fn link_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme, disabled: bool) -> Button {
    Button::new(id).label(label).ghost().with_size(gpui_component::Size::Small).px(px(0.0)).text_color(theme.accent).text_size(gpui::rems(TEXT_SM / 16.0)).disabled(disabled)
}
