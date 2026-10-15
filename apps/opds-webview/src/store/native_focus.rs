//! Route keyboard focus between the GTK browser surface and GPUI controls.
use gdkx11::X11Window;
use gtk::{Window, prelude::*};
use std::cell::Cell;
use std::rc::Rc;
use wry::{WebView, WebViewExtUnix};
use x11rb::{
    CURRENT_TIME,
    connection::Connection,
    protocol::{
        Event,
        xproto::{ChangeWindowAttributesAux, ConnectionExt, EventMask, InputFocus},
    },
    rust_connection::RustConnection,
};

pub(super) struct NativeFocus {
    connection: Rc<RustConnection>,
    child: u32,
    parent: u32,
    page_selected: Rc<Cell<bool>>,
    focused: bool,
    dirty: bool,
}

impl NativeFocus {
    pub(super) fn attach(page: &WebView, parent: u32) -> Option<Self> {
        let view = page.webview();
        let root = view.toplevel()?.downcast::<Window>().ok()?;
        let native = root.window()?;
        let child = native.downcast_ref::<X11Window>()?.xid() as u32;
        let (connection, _) = x11rb::connect(None).ok()?;
        let connection = Rc::new(connection);
        // The clipping surface between GTK and GPUI never owns keyboard focus.
        // Toolbar keys and activation go directly to the supplied GPUI window.
        for window in [child, parent] {
            connection.change_window_attributes(window, &ChangeWindowAttributesAux::new().event_mask(EventMask::FOCUS_CHANGE)).ok()?.check().ok()?;
        }
        let page_selected = Rc::new(Cell::new(false));
        let click_selected = page_selected.clone();
        let click_connection = connection.clone();
        view.connect_button_press_event(move |view, event| {
            click_selected.set(true);
            let _ = click_connection.set_input_focus(InputFocus::PARENT, child, event.time());
            let _ = click_connection.flush();
            view.grab_focus();
            gtk::glib::Propagation::Proceed
        });
        Some(Self { connection, child, parent, page_selected, focused: false, dirty: true })
    }

    pub(super) fn page(&self) {
        self.page_selected.set(true);
        self.focus(self.child);
    }
    pub(super) fn toolbar(&self) {
        self.page_selected.set(false);
        self.focus(self.parent);
    }

    fn focus(&self, window: u32) {
        let _ = self.connection.set_input_focus(InputFocus::PARENT, window, CURRENT_TIME);
        let _ = self.connection.flush();
    }

    /// Returns a page focus transition so GPUI can release its logical focus.
    pub(super) fn sync(&mut self) -> Option<bool> {
        while let Ok(Some(event)) = self.connection.poll_for_event() {
            if matches!(event, Event::FocusIn(_) | Event::FocusOut(_)) {
                self.dirty = true;
            }
        }
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let reply = self.connection.get_input_focus().ok()?.reply().ok()?;
        let mut window = reply.focus;
        // A window manager restores focus to the GPUI parent when returning
        // from another app. Restore the last selected page unless the user
        // clicked the toolbar, which explicitly selects the parent instead.
        if window == self.parent && self.page_selected.get() {
            self.focus(self.child);
            window = self.connection.get_input_focus().ok()?.reply().ok()?.focus;
        }
        // Include native WebKit children (for example a focused popup widget).
        let mut focused = window == self.child;
        for _ in 0..32 {
            if focused || window == self.parent || window <= 1 {
                break;
            }
            let Ok(cookie) = self.connection.query_tree(window) else {
                break;
            };
            let Ok(tree) = cookie.reply() else {
                break;
            };
            if tree.parent == window || tree.parent == tree.root {
                break;
            }
            window = tree.parent;
            focused = window == self.child;
        }
        if focused == self.focused {
            return None;
        }
        self.focused = focused;
        // The GTK-owned surface receives native focus events directly. No
        // synthetic GTK event or JavaScript focus override is needed.
        Some(focused)
    }
}
