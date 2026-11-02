//! Browser navigation, library switching, and route ownership.

use gpui::SharedString;

mod folder_creation;
mod libraries;
mod library_errors;
mod library_session;
mod route;
pub(crate) mod shell;
mod sidebar;

pub(crate) use libraries::Libraries;
pub(crate) use library_session::LibrarySession;
pub use route::{GlobalRoute, LibraryRoute, Route};
pub use shell::BrowserShell;

pub(crate) type Navigate = std::rc::Rc<dyn Fn(Route, &mut gpui::Window, &mut gpui::App)>;

/// The browse views the compact Library tab holds, in menu order.
pub(crate) const LIBRARY_VIEWS: [LibraryRoute; 6] = [LibraryRoute::Books, LibraryRoute::Folders, LibraryRoute::Subjects, LibraryRoute::Authors, LibraryRoute::Search, LibraryRoute::Notebook];

/// The compact heading names the current location and opens the other views.
fn library_view_heading(current: LibraryRoute, title: impl Into<SharedString>, opds: bool, theme: ui_components::BrowserTheme) -> gpui::Div {
    use gpui_component::menu::DropdownMenu as _;
    let title = ui_components::mobile_navigation_menu_title("library-view-menu", title, theme).dropdown_menu_with_anchor(gpui::Anchor::TopLeft, move |mut menu, _, cx| {
        menu = menu.touch_friendly(ui_components::touch_ui());
        for view in LIBRARY_VIEWS.into_iter().filter(|view| !opds || view.available_in_opds()).filter(|view| *view != LibraryRoute::Search || crate::stores::preferences::text_search_enabled(cx)) {
            let (label, icon) = Route::Library(view).presentation();
            menu = menu.item(ui_components::menu_item_with_icon(label, ui_components::navigation_icon(icon), move |_, window, cx| window.dispatch_action(Box::new(crate::OpenLibraryView(view)), cx)).checked(view == current));
        }
        menu
    });
    ui_components::mobile_navigation_menu_heading(title, mobile_back(true))
}

/// The mobile heading at the root of a library page, whether the page shows its
/// books or the empty library.
pub(crate) fn library_root_heading(route: LibraryRoute, opds: bool, theme: ui_components::BrowserTheme) -> gpui::Div {
    let (title, _) = Route::Library(route).presentation();
    // Home is the root of the mobile tab bar: there is nothing to go back to.
    if route == LibraryRoute::Home {
        return ui_components::mobile_navigation_heading(title, None, theme);
    }
    library_heading(route, title, opds, theme)
}

/// Keep the view switcher available while browsing within a library view.
pub(crate) fn library_heading(route: LibraryRoute, title: impl Into<SharedString>, opds: bool, theme: ui_components::BrowserTheme) -> gpui::Div {
    if LIBRARY_VIEWS.contains(&route) {
        return library_view_heading(route, title, opds, theme);
    }
    ui_components::mobile_navigation_heading(title, mobile_back(true), theme)
}

/// The mobile heading's Back, which dismisses like Escape. Root pages have
/// nothing to go back to, and native mobile already has the OS back button.
pub(crate) fn mobile_back(can_go_back: bool) -> Option<ui_components::Button> {
    (can_go_back && !ui_components::has_native_back_button()).then(|| ui_components::mobile_back_button().on_click(|_, window, cx| window.dispatch_action(Box::new(crate::DismissSettings), cx)))
}
