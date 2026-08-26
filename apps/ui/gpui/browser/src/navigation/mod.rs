//! Application navigation state, rail, and library switcher.

use std::rc::Rc;

use app_contract::library::LibrarySummary;
use gpui::prelude::*;
use gpui::{Context, Edges, Entity, EventEmitter, FileDialogFilter, IntoElement, MouseButton, PathPromptOptions, Pixels, PromptLevel, Render, SharedString, Subscription, Window};
use gpui_component::notification::Notification;
use gpui_component::{Icon, IconName, WindowExt};
use sync_common::LibraryId;
use ui_components as components;

use crate::{FocusNextListItem, FocusPreviousListItem};
use app_contract::backend::AppBackend;

const PRIMARY_ROUTES: [Route; 3] = [Route::Home, Route::Folders, Route::Subjects];
const SECONDARY_ROUTES: [Route; 4] = [Route::Authors, Route::Trash, Route::Store, Route::Settings];

/// The currently displayed browser page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Authors,
    #[default]
    Folders,
    Subjects,
    Trash,
    Store,
    /// Hosts the account and sync state as its first section; there is no
    /// separate account destination.
    Settings,
}

/// Observable application-navigation state shared by the root, rail, and Sync.
pub(crate) struct NavigationState {
    route: Route,
    entries: Vec<LibrarySummary>,
    library_id: LibraryId,
}

impl NavigationState {
    pub(crate) fn new(route: Route, entries: Vec<LibrarySummary>, library_id: LibraryId) -> Self {
        Self { route, entries, library_id }
    }

    pub(crate) fn route(&self) -> Route {
        self.route
    }

    pub(crate) fn entries(&self) -> &[LibrarySummary] {
        &self.entries
    }

    pub(crate) fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub(crate) fn set_route(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.route == route {
            return;
        }
        self.route = route;
        cx.notify();
    }

    pub(crate) fn select_library(&mut self, entries: Vec<LibrarySummary>, library_id: LibraryId, route: Route, cx: &mut Context<Self>) {
        self.entries = entries;
        self.library_id = library_id;
        self.route = route;
        cx.notify();
    }

    pub(crate) fn set_entries(&mut self, entries: Vec<LibrarySummary>, cx: &mut Context<Self>) {
        let unchanged = self.entries.len() == entries.len() && self.entries.iter().zip(&entries).all(|(left, right)| left.library_id() == right.library_id() && left.library_name() == right.library_name());
        if unchanged {
            return;
        }
        self.entries = entries;
        cx.notify();
    }
}

#[derive(Clone)]
pub(crate) enum NavigationRequest {
    Select(Route),
    SelectLibrary(LibraryId),
}

pub(crate) struct NavigationView {
    backend: Rc<dyn AppBackend>,
    import_extensions: Vec<String>,
    state: Entity<NavigationState>,
    switcher_open: bool,
    more_open: bool,
    _state_subscription: Subscription,
}

#[derive(Clone)]
struct LibraryChoice {
    id: LibraryId,
    name: SharedString,
}

fn route_definition(route: Route) -> (&'static str, Icon) {
    use app_contract::NavigationIcon;
    let (label, icon) = match route {
        Route::Home => ("Home", NavigationIcon::House),
        Route::Authors => ("Authors", NavigationIcon::User),
        Route::Folders => ("Folders", NavigationIcon::Folder),
        Route::Subjects => ("Subjects", NavigationIcon::Tags),
        Route::Trash => ("Trash", NavigationIcon::Trash2),
        Route::Store => ("Store", NavigationIcon::Building2),
        Route::Settings => ("Settings", NavigationIcon::Settings),
    };
    (label, components::navigation_icon(icon))
}

impl EventEmitter<NavigationRequest> for NavigationView {}

impl NavigationView {
    pub(crate) fn new(backend: Rc<dyn AppBackend>, import_extensions: Vec<String>, state: Entity<NavigationState>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&state, |_: &mut Self, _, cx| cx.notify());
        Self { backend, import_extensions, state, switcher_open: false, more_open: false, _state_subscription: subscription }
    }

    pub(crate) fn is_library_switcher_open(&self) -> bool {
        self.switcher_open
    }

    pub(crate) fn close_library_switcher(&mut self, cx: &mut Context<Self>) {
        if !self.switcher_open {
            return;
        }
        self.switcher_open = false;
        cx.notify();
    }

    pub(crate) fn library_switcher_overlay(&self, emitter: Entity<Self>, cx: &gpui::App) -> Option<gpui::AnyElement> {
        self.switcher_open.then(|| self.render_library_switcher_overlay(emitter, cx))
    }

    pub(crate) fn is_more_open(&self) -> bool {
        self.more_open
    }

    pub(crate) fn close_more(&mut self, cx: &mut Context<Self>) {
        if !self.more_open {
            return;
        }
        self.more_open = false;
        cx.notify();
    }

    pub(crate) fn more_overlay(&self, emitter: Entity<Self>, safe_area: Edges<Pixels>, cx: &gpui::App) -> Option<gpui::AnyElement> {
        self.more_open.then(|| self.render_more_overlay(emitter, safe_area, cx))
    }

    #[cfg(feature = "filesystem-libraries")]
    fn choose_library_folders(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths_with_filters(
            PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Choose library folders".into()) },
            vec![FileDialogFilter { name: "Books".to_owned(), extensions: self.import_extensions.clone() }],
        );
        let backend = self.backend.clone();
        cx.spawn(async move |navigation, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            if paths.is_empty() {
                return;
            }
            let result = backend.create_libraries_from_paths(paths).await;
            let _ = navigation.update_in(cx, |_, window, cx| match result {
                Ok((created, errors)) => {
                    if !errors.is_empty() {
                        window.push_notification(Notification::warning(format!("Some folders could not be added: {}", errors.join("; "))).id1::<NavigationView>("library-add-warning"), cx);
                    }
                    if let Some(library_id) = created.last() {
                        cx.emit(NavigationRequest::SelectLibrary(*library_id));
                    }
                }
                Err(error) => {
                    window.push_notification(Notification::error(format!("Could not add library folders: {error}")).id1::<NavigationView>("library-add-error").autohide(false), cx);
                }
            });
        })
        .detach();
    }

    #[cfg(not(feature = "filesystem-libraries"))]
    fn choose_library_folders(&mut self, cx: &mut Context<Self>) {
        let directories = cx.prompt_for_directories_with_filters(
            PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Choose library folders".into()) },
            vec![FileDialogFilter { name: "Books".to_owned(), extensions: self.import_extensions.clone() }],
        );
        let backend = self.backend.clone();
        cx.spawn(async move |navigation, cx| {
            let Ok(Ok(Some(directories))) = directories.await else { return };
            if directories.is_empty() {
                return;
            }
            let mut created = Vec::new();
            let mut errors = Vec::new();
            for directory in directories.into_iter().map(crate::directory_import) {
                let name = directory.name.clone();
                match backend.create_library(name.clone()).await {
                    Ok(library_id) => created.push((library_id, name, directory)),
                    Err(error) => errors.push(format!("{name}: {error}")),
                }
            }
            let selected = created.last().map(|(library_id, _, _)| *library_id);
            let _ = navigation.update_in(cx, |_, window, cx| {
                if !errors.is_empty() {
                    window.push_notification(Notification::warning(format!("Some folders could not be added: {}", errors.join("; "))).id1::<NavigationView>("library-add-warning"), cx);
                }
                if let Some(library_id) = selected {
                    cx.emit(NavigationRequest::SelectLibrary(library_id));
                }
            });
            if created.is_empty() {
                return;
            }
            let imports = created.into_iter().map(|(library_id, name, directory)| {
                let backend = backend.clone();
                async move { (name, backend.import_library_directory(library_id, directory).await) }
            });
            let mut import_errors = Vec::new();
            for (name, result) in futures_util::future::join_all(imports).await {
                match result {
                    Ok(failures) => import_errors.extend(failures.into_iter().map(|failure| format!("{name}/{failure}"))),
                    Err(error) => import_errors.push(format!("{name}: {error}")),
                }
            }
            if !import_errors.is_empty() {
                let _ = navigation.update_in(cx, |_, window, cx| {
                    window.push_notification(Notification::warning(format!("Some books could not be imported: {}", import_errors.join("; "))).id1::<NavigationView>("library-import-warning").autohide(false), cx);
                });
            }
        })
        .detach();
    }

    fn libraries(&self, cx: &gpui::App) -> Vec<LibraryChoice> {
        let mut choices = self.state.read(cx).entries().iter().map(|library| LibraryChoice { id: *library.library_id(), name: library.library_name().to_owned().into() }).collect::<Vec<_>>();
        choices.sort_by_cached_key(|library| library.name.to_lowercase());
        choices
    }

    fn active_library_name(&self, cx: &gpui::App) -> SharedString {
        let state = self.state.read(cx);
        state.entries().iter().find(|library| library.library_id() == state.library_id()).map(|library| library.library_name().to_owned().into()).unwrap_or_else(|| "Library".into())
    }

    fn render_library_switcher_overlay(&self, emitter: Entity<Self>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let active_library_id = *self.state.read(cx).library_id();
        let libraries = self.libraries(cx);
        let library_count = libraries.len();
        let add = emitter.clone();
        let mut panel =
            components::library_switcher_panel().key_context("BrowserArrowList").on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx)).on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx)).child(
                components::library_switcher_header("Libraries", theme).child(components::library_switcher_add_button(theme).on_click(move |_, _, cx| {
                    add.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.notify();
                        navigation.choose_library_folders(cx);
                    });
                })),
            );

        for library in libraries {
            let library_id = library.id;
            let selected = active_library_id == library_id;
            let select = emitter.clone();
            let select_id = library_id;
            let remove = emitter.clone();
            let remove_id = library_id;
            panel = panel.child(components::library_switcher_row(
                components::library_switcher_item(format!("library-switcher-{library_id}"), library.name.clone(), selected).on_click(move |_, _, cx| {
                    select.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.notify();
                        cx.emit(NavigationRequest::SelectLibrary(select_id));
                    });
                }),
                components::library_switcher_delete_button(format!("library-switcher-delete-{library_id}"), library.name, theme).on_click(move |_, window, cx| {
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        "Delete this library?",
                        Some("Local database and state are removed immediately. Server deletion happens now when possible, or automatically after the next sign-in. Local files and stored server blobs are kept."),
                        &["Delete library", "Cancel"],
                        cx,
                    );
                    remove.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.notify();
                        let backend = navigation.backend.clone();
                        cx.spawn(async move |navigation, cx| {
                            if answer.await.ok() != Some(0) {
                                return;
                            }
                            if let Err(error) = backend.delete_library(remove_id).await {
                                let _ = navigation.update_in(cx, |_, window, cx| {
                                    window.push_notification(Notification::error(error).id1::<NavigationView>("library-removal-error"), cx);
                                });
                            }
                        })
                        .detach();
                    });
                }),
            ));
        }

        let dismiss = emitter;
        gpui::deferred(
            components::library_switcher_overlay()
                .child(components::library_switcher_scrim(theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss.update(cx, |navigation, cx| navigation.close_library_switcher(cx));
                }))
                .child(components::library_switcher_dialog(library_count, theme).child(panel)),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn render_library_switcher_trigger(&self, emitter: Entity<Self>, cx: &gpui::App) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        components::library_switcher_group(theme).child(components::library_switcher_trigger(self.active_library_name(cx), theme).on_click(move |_, _, cx| {
            emitter.update(cx, |navigation, cx| {
                if !navigation.switcher_open {
                    navigation.switcher_open = true;
                    cx.notify();
                }
            });
        }))
    }

    fn render_more_overlay(&self, emitter: Entity<Self>, safe_area: Edges<Pixels>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let route = self.state.read(cx).route();
        let item_count = SECONDARY_ROUTES.len() + 1 + usize::from(cfg!(feature = "kobo"));
        let open_libraries = emitter.clone();
        let mut items = gpui::div().w_full().flex().flex_col().gap(gpui::px(components::SPACE_XS)).child(components::more_navigation_header(theme)).child(
            components::more_navigation_item("more-libraries", components::navigation_icon(app_contract::NavigationIcon::BookOpen), self.active_library_name(cx), false, theme).on_click(move |_, _, cx| {
                open_libraries.update(cx, |navigation, cx| {
                    navigation.more_open = false;
                    navigation.switcher_open = true;
                    cx.notify();
                });
            }),
        );
        for target in SECONDARY_ROUTES {
            let (id, icon) = route_definition(target);
            let label = SharedString::from(id);
            let select = emitter.clone();
            items = items.child(components::more_navigation_item(format!("more-{id}"), icon, label, route == target, theme).on_click(move |_, _, cx| {
                select.update(cx, |navigation, cx| {
                    navigation.more_open = false;
                    cx.notify();
                    cx.emit(NavigationRequest::Select(target));
                });
            }));
        }
        if cfg!(feature = "kobo") {
            items = items.child(components::more_navigation_item("more-exit-kobo", IconName::WindowClose, "Exit to Kobo", false, theme).on_click(|_, _, cx| cx.quit()));
        }
        let dismiss = emitter;
        gpui::deferred(
            components::more_navigation_overlay()
                .pt(safe_area.top)
                .pr(safe_area.right)
                .pb(safe_area.bottom)
                .pl(safe_area.left)
                .child(components::more_navigation_scrim(theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss.update(cx, |navigation, cx| navigation.close_more(cx));
                }))
                .child(components::more_navigation_sheet(item_count, theme).child(items)),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn render_bottom_navigation(&self, emitter: Entity<Self>, safe_area: Edges<Pixels>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let route = self.state.read(cx).route();
        let mut navigation = components::bottom_navigation(theme);
        for target in PRIMARY_ROUTES {
            let (id, icon) = route_definition(target);
            let select = emitter.clone();
            navigation = navigation.child(components::bottom_navigation_item(format!("bottom-{id}"), icon, id, route == target, theme).on_click(move |_, _, cx| {
                select.update(cx, |navigation, cx| {
                    navigation.more_open = false;
                    cx.emit(NavigationRequest::Select(target));
                });
            }));
        }
        let more_selected = self.more_open || SECONDARY_ROUTES.contains(&route);
        let navigation = navigation.child(components::bottom_navigation_item("bottom-more", IconName::Ellipsis, "More", more_selected, theme).on_click(move |_, _, cx| {
            emitter.update(cx, |navigation, cx| {
                navigation.more_open = !navigation.more_open;
                cx.notify();
            });
        }));
        gpui::div().w_full().flex_none().flex().flex_col().pl(safe_area.left).pr(safe_area.right).bg(theme.page_bg).child(navigation).child(gpui::div().w_full().h(safe_area.bottom).flex_none()).into_any_element()
    }

    fn render_sidebar(&self, emitter: Entity<Self>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let route = self.state.read(cx).route();
        let mut sidebar = components::sidebar(theme).child(self.render_library_switcher_trigger(emitter.clone(), cx));
        for target in [Route::Home, Route::Folders, Route::Subjects, Route::Authors, Route::Trash, Route::Store, Route::Settings] {
            if target == Route::Store {
                sidebar = sidebar.child(gpui::div().flex_1().min_h_0()).child(components::nav_rail_divider(theme));
                if cfg!(feature = "kobo") {
                    sidebar = sidebar.child(components::nav_rail_item("exit-kobo", IconName::WindowClose, "Exit to Kobo", false, theme).on_click(|_, _, cx| cx.quit()));
                }
            }
            let (id, icon) = route_definition(target);
            let label = SharedString::from(id);
            let select = emitter.clone();
            sidebar = sidebar.child(components::nav_rail_item(id, icon, label, route == target, theme).on_click(move |_, _, cx| {
                select.update(cx, |_, cx| cx.emit(NavigationRequest::Select(target)));
            }));
        }
        gpui::div().w(gpui::rems(components::NAV_RAIL_SIZE_REM)).h_full().flex_none().child(sidebar).into_any_element()
    }
}

impl Render for NavigationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let emitter = cx.entity();
        let safe_area = window.insets().safe_area;
        if components::WindowWidthClass::for_window(window).is_compact() {
            self.render_bottom_navigation(emitter, safe_area, cx)
        } else {
            self.more_open = false;
            self.render_sidebar(emitter, cx)
        }
    }
}
