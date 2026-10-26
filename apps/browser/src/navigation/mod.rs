//! Browser navigation, library switching, and route ownership.

mod folder_creation;
mod libraries;
mod library_errors;
mod library_session;
mod route;
pub(crate) mod shell;
mod sidebar;

pub(crate) use libraries::Libraries;
pub(crate) use library_session::LibrarySession;
pub(crate) use route::LibrarySummary;
pub use route::{GlobalRoute, LibraryRoute, Route};
pub use shell::BrowserShell;

pub(crate) type Navigate = std::rc::Rc<dyn Fn(Route, &mut gpui::Window, &mut gpui::App)>;

/// The browse views the phone's Library tab holds, in the order its menu lists
/// them. A phone's tab bar has no room for each of them as a tab.
pub(crate) const LIBRARY_VIEWS: [LibraryRoute; 4] = [LibraryRoute::Books, LibraryRoute::Folders, LibraryRoute::Subjects, LibraryRoute::Authors];

/// The mobile heading at a library view's root. Its title is the view's name
/// and opens a menu of the other views, which is how a phone moves between them.
pub(crate) fn library_view_heading(current: LibraryRoute, theme: ui_components::BrowserTheme) -> gpui::Div {
    use gpui_component::menu::DropdownMenu as _;
    let (title, _) = sidebar::route_definition(Route::Library(current));
    let title = ui_components::mobile_navigation_menu_title("library-view-menu", title, theme).dropdown_menu_with_anchor(gpui::Anchor::TopLeft, move |mut menu, _, _| {
        menu = menu.touch_friendly(true);
        for view in LIBRARY_VIEWS {
            let (label, icon) = sidebar::route_definition(Route::Library(view));
            menu = menu.item(ui_components::menu_item_with_icon(label, icon, move |_, window, cx| window.dispatch_action(Box::new(crate::OpenLibraryView(view)), cx)).checked(view == current));
        }
        menu
    });
    ui_components::mobile_navigation_menu_heading(title, mobile_back(true, theme))
}

/// The mobile heading at the root of a library page, whether the page shows its
/// books or the empty library.
pub(crate) fn library_root_heading(route: LibraryRoute, theme: ui_components::BrowserTheme) -> gpui::Div {
    if ui_components::touch_ui() && LIBRARY_VIEWS.contains(&route) {
        return library_view_heading(route, theme);
    }
    let (title, _) = sidebar::route_definition(Route::Library(route));
    // Home is the root of the mobile tab bar: there is nothing to go back to.
    ui_components::mobile_navigation_heading(title, mobile_back(route != LibraryRoute::Home, theme), theme)
}

/// The mobile heading's Back, which dismisses like Escape. Root pages have
/// nothing to go back to, and native mobile already has the OS back button.
pub(crate) fn mobile_back(can_go_back: bool, theme: ui_components::BrowserTheme) -> Option<gpui_component::button::Button> {
    (can_go_back && !ui_components::has_native_back_button()).then(|| ui_components::mobile_back_button(theme).on_click(|_, window, cx| window.dispatch_action(Box::new(crate::DismissSettings), cx)))
}
