//! Keep a real GTK-owned browser surface inside the GPUI window.
use anyhow::{Context, Result, bail};
use gdkx11::X11Window;
use gpui::{Bounds, Pixels};
use gtk::{Box as GtkBox, Orientation, Window, WindowType, prelude::*};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::RefCell;
use std::collections::BTreeMap;
use wry::Rect;
use x11rb::{
    COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, NONE,
    connection::Connection,
    protocol::{
        shape::{ConnectionExt as ShapeConnectionExt, SK, SO},
        xproto::{ClipOrdering, ConfigureWindowAux, ConnectionExt, CreateWindowAux, Rectangle, WindowClass},
    },
    rust_connection::RustConnection,
};

pub(super) struct NativeHost {
    window: Window,
    pub(super) container: GtkBox,
    connection: RustConnection,
    child: u32,
    clip: u32,
    parent: u32,
    overlays: RefCell<BTreeMap<&'static str, Bounds<Pixels>>>,
    mask: RefCell<Option<OverlayMask>>,
}

#[derive(PartialEq)]
struct OverlayMask {
    width: u16,
    height: u16,
    holes: Vec<(i16, i16, u16, u16)>,
}

impl NativeHost {
    pub(super) fn new(parent: &impl HasWindowHandle, bounds: Rect) -> Result<Self> {
        let parent = match parent.window_handle()?.as_raw() {
            RawWindowHandle::Xlib(handle) => handle.window as u32,
            _ => bail!("The native browser surface needs an X11 parent"),
        };
        let window = Window::new(WindowType::Toplevel);
        window.set_decorated(false);
        window.set_skip_taskbar_hint(true);
        window.set_skip_pager_hint(true);
        let container = GtkBox::new(Orientation::Vertical, 0);
        window.add(&container);
        window.realize();
        let native = window.window().context("GTK did not create a browser window")?;
        native.set_override_redirect(true);
        native.display().sync();
        let child = native.downcast_ref::<X11Window>().context("The browser surface is not X11")?.xid() as u32;
        let (connection, _) = x11rb::connect(None)?;
        // Keep GDK's real top-level window and event handling. Only change the
        // X11 parent, so GTK draws within GPUI without creating another window.
        let clip = connection.generate_id()?;
        connection.create_window(COPY_DEPTH_FROM_PARENT, clip, parent, 0, 0, 1, 1, 0,
            WindowClass::INPUT_OUTPUT, COPY_FROM_PARENT, &CreateWindowAux::new().background_pixel(0))?.check()?;
        connection.reparent_window(child, clip, 0, 0)?.check()?;
        let host = Self { window, container, connection, child, clip, parent, overlays: RefCell::default(), mask: RefCell::default() };
        host.set_bounds(bounds)?;
        Ok(host)
    }

    pub(super) fn show(&self) {
        self.window.show_all();
        let _ = self.connection.map_window(self.clip);
        let _ = self.connection.flush();
    }

    pub(super) fn parent(&self) -> u32 { self.parent }

    pub(super) fn set_bounds(&self, bounds: Rect) -> Result<()> {
        let position = bounds.position.to_physical::<i32>(1.0);
        let size = bounds.size.to_physical::<u32>(1.0);
        let scale = self.window.scale_factor();
        self.window.size_allocate(&gtk::Allocation::new(0, 0, (size.width as i32 / scale).max(1), (size.height as i32 / scale).max(1)));
        self.connection.configure_window(self.clip, &ConfigureWindowAux::new().x(position.x).y(position.y).width(size.width.max(1)).height(size.height.max(1)))?.check()?;
        self.connection.configure_window(self.child, &ConfigureWindowAux::new().width(size.width.max(1)).height(size.height.max(1)))?.check()?;
        self.connection.flush()?;
        Ok(())
    }

    /// Shape an X11 container rather than GTK's own window: GTK updates its
    /// shape during painting and would otherwise overwrite our dropdown hole.
    /// The GTK surface retains its native focus events and full viewport.
    pub(super) fn set_overlay(&self, key: &'static str, page: Rect, overlay: Option<Bounds<Pixels>>, scale: f32) {
        let mut overlays = self.overlays.borrow_mut();
        if let Some(bounds) = overlay { overlays.insert(key, bounds); } else { overlays.remove(key); }
        let size = page.size.to_physical::<u32>(1.0);
        let position = page.position.to_physical::<i32>(1.0);
        let width = size.width.min(u16::MAX as u32) as u16;
        let height = size.height.min(u16::MAX as u32) as u16;
        let holes: Vec<_> = overlays.values().filter_map(|bounds| {
            // Include the border outside the measurement canvas.
            let left = (f32::from(bounds.left()) * scale - position.x as f32 - scale).floor().clamp(0.0, width as f32) as i16;
            let top = (f32::from(bounds.top()) * scale - position.y as f32 - scale).floor().clamp(0.0, height as f32) as i16;
            let right = (f32::from(bounds.right()) * scale - position.x as f32 + scale).ceil().clamp(left as f32, width as f32) as u16;
            let bottom = (f32::from(bounds.bottom()) * scale - position.y as f32 + scale).ceil().clamp(top as f32, height as f32) as u16;
            let hole_width = right - left as u16;
            let hole_height = bottom - top as u16;
            (hole_width > 0 && hole_height > 0).then_some((left, top, hole_width, hole_height))
        }).collect();
        let mask = OverlayMask { width, height, holes };
        if self.mask.borrow().as_ref() == Some(&mask) { return; }
        let result = (|| -> Result<()> {
            if !mask.holes.is_empty() {
                let holes: Vec<_> = mask.holes.iter().map(|&(x, y, width, height)| Rectangle { x, y, width, height }).collect();
                self.connection.shape_rectangles(SO::SET, SK::BOUNDING, ClipOrdering::UNSORTED, self.clip, 0, 0,
                    &[Rectangle { x: 0, y: 0, width, height }])?.check()?;
                self.connection.shape_rectangles(SO::SUBTRACT, SK::BOUNDING, ClipOrdering::UNSORTED, self.clip, 0, 0,
                    &holes)?.check()?;
            } else {
                self.connection.shape_mask(SO::SET, SK::BOUNDING, self.clip, 0, 0, NONE)?.check()?;
            }
            self.connection.flush()?;
            Ok(())
        })();
        match result {
            Ok(()) => *self.mask.borrow_mut() = Some(mask),
            Err(error) => eprintln!("Could not reveal browser overlays: {error}"),
        }
    }

}

impl Drop for NativeHost {
    fn drop(&mut self) {
        // Destroy GTK's window synchronously before its X11 parent. close()
        // queues an event and would leave GDK observing a destroyed child.
        let display = self.window.display();
        unsafe { self.window.destroy(); }
        // GDK and the clipping surface use separate X11 connections.
        display.sync();
        let _ = self.connection.destroy_window(self.clip);
        let _ = self.connection.flush();
    }
}
