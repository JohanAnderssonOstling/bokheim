//! Activation for pages that hold a paginator.

use gpui::{App, Entity, Window};

use super::Paginator;

/// Resolve activation after the current page update has released its entities.
pub(crate) fn activate_paginator(paginator: &Entity<Paginator>, window: &mut Window, cx: &mut App) {
    let paginator = paginator.clone();
    window.defer(cx, move |window, cx| {
        if let Some(activate) = paginator.read(cx).cursor_activation() {
            activate(window, cx);
        }
    });
}
