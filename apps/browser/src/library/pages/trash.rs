//! Library Trash. Deleted items remain recoverable here until explicitly purged.
//!
//! One list, newest first, folders and books together and grouped by how long
//! ago they were deleted: the question Trash is opened with is "what did I
//! just delete?". Items are selected to restore or delete several at once.

use ui_components::user_facing_error;

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{Context, Entity, FocusHandle, Focusable, IntoElement, Render, SharedString, Subscription, Task, Window, div};
use gpui_component::IconName;
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{InputEvent, InputState};
use gpui_image_eviction::track_image;
use library_backend::{LibraryClient, ThumbnailResolution};
use library_model::{CardKey, LibraryTrashView, TrashedBook, TrashedFolder};
use sync_common::{BookId, DirId};
use ui_components as components;
use web_time::{SystemTime, UNIX_EPOCH};

use crate::library::pages::with_retry_banner;
use crate::library::services::{LibraryContentsChanged, LibraryContext};
use crate::library::widgets::{CoversEvicted, LoadedCover, cover_generation, generated_cover_variant, loaded_cover_from_jpeg};

const DAY_MS: i64 = 86_400_000;

/// The groups the list is split into, each holding what was deleted less
/// than its age ago and not already in an earlier group. Ages rather than
/// calendar days: the page has no time zone to draw midnight with.
const GROUPS: [(&str, i64); 3] = [("Past day", DAY_MS), ("Past week", 7 * DAY_MS), ("Earlier", i64::MAX)];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum TrashKey {
    Folder(DirId),
    Book(BookId),
}

impl std::fmt::Display for TrashKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Folder(id) => write!(formatter, "folder-{id}"),
            Self::Book(content_hash) => write!(formatter, "book-{content_hash}"),
        }
    }
}

#[derive(Clone)]
enum TrashItem {
    Folder(TrashedFolder),
    Book(TrashedBook),
}

impl TrashItem {
    fn key(&self) -> TrashKey {
        match self {
            Self::Folder(folder) => TrashKey::Folder(folder.id),
            Self::Book(book) => TrashKey::Book(book.content_hash),
        }
    }

    fn deleted_at(&self) -> i64 {
        match self {
            Self::Folder(folder) => folder.deleted_at,
            Self::Book(book) => book.deleted_at,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Folder(folder) => &folder.name,
            Self::Book(book) => &book.title,
        }
    }

    /// Whether a lowercased search matches the name, or a book's author.
    fn matches(&self, query: &str) -> bool {
        let found = |text: &str| text.to_lowercase().contains(query);
        match self {
            Self::Folder(folder) => found(&folder.name),
            Self::Book(book) => found(&book.title) || found(&book.author),
        }
    }

    fn description(&self, now: i64) -> String {
        let ago = deleted_ago(self.deleted_at(), now);
        match self {
            Self::Folder(folder) => match &folder.parent_name {
                Some(parent) => format!("{parent} · {ago}"),
                None => ago,
            },
            Self::Book(book) => [book.author.as_str(), book.format.to_uppercase().as_str(), ago.as_str()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · "),
        }
    }
}

/// Folders and books together, newest first.
fn trash_items(contents: &LibraryTrashView) -> Vec<TrashItem> {
    let mut items: Vec<TrashItem> = contents.folders.iter().cloned().map(TrashItem::Folder).chain(contents.books.iter().cloned().map(TrashItem::Book)).collect();
    items.sort_by_key(|item| Reverse(item.deleted_at()));
    items
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// How long ago, in the coarsest unit that still tells items apart.
fn deleted_ago(deleted_at: i64, now: i64) -> String {
    let minutes = (now - deleted_at).max(0) / 60_000;
    let days = minutes / (24 * 60);
    match minutes {
        0 => "deleted just now".to_owned(),
        1..60 => format!("deleted {minutes} min ago"),
        60..1440 => format!("deleted {} h ago", minutes / 60),
        _ if days == 1 => "deleted a day ago".to_owned(),
        _ if days < 60 => format!("deleted {days} days ago"),
        _ if days < 730 => format!("deleted {} months ago", days / 30),
        _ => format!("deleted {} years ago", days / 365),
    }
}

fn group_of(deleted_at: i64, now: i64) -> usize {
    let age = now - deleted_at;
    GROUPS.iter().position(|(_, limit)| age < *limit).unwrap_or(GROUPS.len() - 1)
}

fn count_label(items: &[TrashItem]) -> String {
    let folders = items.iter().filter(|item| matches!(item, TrashItem::Folder(_))).count();
    let books = items.len() - folders;
    let plural = |count: usize, one: &str, many: &str| format!("{count} {}", if count == 1 { one } else { many });
    match (folders, books) {
        (0, books) => plural(books, "book", "books"),
        (folders, 0) => plural(folders, "folder", "folders"),
        (folders, books) => format!("{} and {}", plural(folders, "folder", "folders"), plural(books, "book", "books")),
    }
}

pub(crate) struct TrashPage {
    ui: LibraryContext,
    contents: Option<LibraryTrashView>,
    error: Option<SharedString>,
    /// The load in flight. Replacing it drops the one before, which cancels it.
    request: Option<Task<()>>,
    selection: HashSet<TrashKey>,
    /// Loaded covers, and `None` for a book known to have none.
    covers: HashMap<BookId, Option<LoadedCover>>,
    covers_need_reload: bool,
    _cover_evictions: Subscription,
    cover_request: Option<Task<()>>,
    /// Created on first render, since an input needs a window.
    search: Option<Entity<InputState>>,
    /// The search, lowercased.
    query: String,
    focus: FocusHandle,
    _updates: Subscription,
    _search_changes: Option<Subscription>,
}

impl TrashPage {
    pub(crate) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let updates = ui.updates();
        let update_subscription = cx.subscribe(&updates, |page, _, _: &LibraryContentsChanged, cx| page.refresh(cx));
        let cover_evictions = cx.observe_global::<CoversEvicted>(|page: &mut Self, cx| {
            page.cover_request.take();
            page.covers.clear();
            page.covers_need_reload = true;
            cx.notify();
        });
        let mut page = Self {
            ui,
            contents: None,
            error: None,
            request: None,
            selection: HashSet::new(),
            covers: HashMap::new(),
            covers_need_reload: false,
            _cover_evictions: cover_evictions,
            cover_request: None,
            search: None,
            query: String::new(),
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            _updates: update_subscription,
            _search_changes: None,
        };
        page.refresh(cx);
        page
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let library = self.ui.backend().clone();
        let load = async move { library.trash().await };
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            cx.wait_for_update_slot().await;
            let _ = page.update(cx, |page, cx| {
                match result {
                    Ok(contents) => {
                        let present: HashSet<TrashKey> = trash_items(&contents).iter().map(TrashItem::key).collect();
                        page.selection.retain(|key| present.contains(key));
                        page.contents = Some(contents);
                        page.error = None;
                        // Hidden cached pages must not fetch covers while a
                        // reader is open. Start the load on their next render.
                        page.covers_need_reload = true;
                    }
                    Err(error) => page.error = Some(user_facing_error(&error, "Could not load Trash. Please try again.").into()),
                }
                cx.notify();
            });
        });
        self.request = Some(task);
    }

    /// Loads the covers of books not yet tried, and shows them all at once.
    fn load_covers(&mut self, cx: &mut Context<Self>) {
        let Some(contents) = &self.contents else { return };
        let missing: Vec<BookId> = contents.books.iter().map(|book| book.content_hash).filter(|content_hash| !self.covers.contains_key(content_hash)).collect();
        if missing.is_empty() {
            return;
        }
        // With "generated covers only", every book is drawn generated and no
        // art is read.
        if crate::stores::generated_covers_only(cx) {
            self.covers.extend(missing.into_iter().map(|content_hash| (content_hash, None)));
            cx.notify();
            return;
        }
        let library = self.ui.backend().clone();
        let decoder = cx.background_executor().clone();
        let generation = cover_generation(cx);
        self.cover_request = Some(cx.spawn(async move |page, cx| {
            let mut loaded = Vec::new();
            for content_hash in missing {
                let cover = match library.thumbnail(content_hash, ThumbnailResolution::Browse).await {
                    Ok(Some(bytes)) => {
                        let Some(cover) = decoder.spawn(async move { loaded_cover_from_jpeg(bytes) }).await else {
                            log::error!("thumbnail for trashed book {content_hash} is not a readable JPEG");
                            continue;
                        };
                        Some(cover)
                    }
                    Ok(None) => None,
                    Err(error) => {
                        log::error!("failed to load the thumbnail for trashed book {content_hash}: {error}");
                        continue;
                    }
                };
                loaded.push((content_hash, cover));
            }
            cx.wait_for_update_slot().await;
            let _ = page.update(cx, |page, cx| {
                if generation != cover_generation(cx) {
                    return;
                }
                page.cover_request.take();
                for (_, cover) in &loaded {
                    if let Some(cover) = cover {
                        track_image(&cover.image, cx);
                    }
                }
                page.covers.extend(loaded);
                cx.notify();
            });
        }));
    }

    fn search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(search) = &self.search {
            return search.clone();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search Trash"));
        self._search_changes = Some(cx.subscribe(&search, |page, search, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                page.query = search.read(cx).value().trim().to_lowercase();
                cx.notify();
            }
        }));
        self.search = Some(search.clone());
        search
    }

    /// Runs one Trash action. Success needs no reload here: the library
    /// reports its contents changed, and that refreshes this page.
    fn run<F: Future<Output = Result<(), library_backend::BackendError>> + 'static>(&mut self, operation: impl FnOnce(LibraryClient) -> F, action: &'static str, cx: &mut Context<Self>) {
        let operation = operation(self.ui.backend().clone());
        cx.spawn(async move |page, cx| {
            if let Err(error) = operation.await {
                cx.defer_update(page, move |page, cx| {
                    page.error = Some(format!("Could not {action}. {}", user_facing_error(&error, "Please try again.")).into());
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn restore(&mut self, items: Vec<TrashItem>, cx: &mut Context<Self>) {
        for item in items {
            self.selection.remove(&item.key());
            match item {
                TrashItem::Book(book) => self.run(move |library| async move { library.restore_book(book.content_hash).await }, "restore book", cx),
                TrashItem::Folder(folder) => self.run(move |library| async move { library.restore_directory(folder.id, None).await.map(drop) }, "restore folder", cx),
            }
        }
        cx.notify();
    }

    fn purge(&mut self, items: Vec<TrashItem>, cx: &mut Context<Self>) {
        for item in items {
            self.selection.remove(&item.key());
            match item {
                TrashItem::Book(book) => self.run(move |library| async move { library.purge_book(book.content_hash).await }, "delete book permanently", cx),
                TrashItem::Folder(folder) => self.run(move |library| async move { library.purge_directory(folder.id).await }, "delete folder permanently", cx),
            }
        }
        cx.notify();
    }

    fn empty(&mut self, cx: &mut Context<Self>) {
        self.selection.clear();
        self.run(|library| async move { library.empty_trash().await }, "empty Trash", cx);
        cx.notify();
    }

    /// Asks before deleting for good. `everything` empties Trash in one
    /// command rather than purging the items one by one.
    fn confirm_purge(&mut self, items: Vec<TrashItem>, everything: bool, window: &mut Window, cx: &mut Context<Self>) {
        if items.is_empty() {
            return;
        }
        let title: SharedString = match items.as_slice() {
            [item] => format!("Delete “{}” permanently?", item.name()).into(),
            _ => format!("Delete {} permanently?", count_label(&items)).into(),
        };
        let page = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let page = page.clone();
            let items = items.clone();
            components::confirmation_dialog(dialog, window).title(title.clone()).button_props(DialogButtonProps::default().ok_text("Delete permanently").ok_variant(ButtonVariant::Danger).cancel_text("Cancel").show_cancel(true)).on_ok(
                move |_, _, cx| {
                    let items = items.clone();
                    let _ = page.update(cx, |page, cx| if everything { page.empty(cx) } else { page.purge(items, cx) });
                    true
                },
            )
        });
    }

    fn selected_items(&self) -> Vec<TrashItem> {
        self.contents.as_ref().map(trash_items).unwrap_or_default().into_iter().filter(|item| self.selection.contains(&item.key())).collect()
    }

    fn render_cover(&self, content_hash: BookId, theme: components::BrowserTheme) -> gpui::AnyElement {
        match self.covers.get(&content_hash) {
            Some(Some(cover)) => components::book_cover(theme, components::BOOK_COVER_ASPECT_RATIO).child(components::book_cover_image(format!("trash-cover-{content_hash}"), cover.image.clone(), 1.0, false, theme)).into_any_element(),
            Some(None) => components::book_thumbnail_generated(generated_cover_variant(CardKey::from(content_hash)), theme).into_any_element(),
            None => components::book_cover(theme, components::BOOK_COVER_ASPECT_RATIO).into_any_element(),
        }
    }

    fn render_item(&self, item: &TrashItem, now: i64, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let key = item.key();
        let selected = self.selection.contains(&key);
        let page = cx.entity();
        let checkbox = components::selection_checkbox(format!("trash-select-{key}"), selected, theme).on_click({
            let page = page.clone();
            move |_, _, cx| {
                page.update(cx, |page, cx| {
                    if !page.selection.remove(&key) {
                        page.selection.insert(key);
                    }
                    cx.notify();
                })
            }
        });
        let leading = match item {
            TrashItem::Folder(_) => components::trash_folder_thumbnail(theme),
            TrashItem::Book(book) => components::trash_book_thumbnail(self.render_cover(book.content_hash, theme)),
        };
        let restore = components::topbar_action_button(format!("trash-restore-{key}"), "Restore", IconName::Undo2, theme).on_click({
            let page = page.clone();
            let item = item.clone();
            move |_, _, cx| page.update(cx, |page, cx| page.restore(vec![item.clone()], cx))
        });
        let delete = components::delete_icon_button(format!("trash-delete-{key}"), "Delete permanently", theme).on_click({
            let item = item.clone();
            move |_, window, cx| page.update(cx, |page, cx| page.confirm_purge(vec![item.clone()], false, window, cx))
        });
        components::trash_row(checkbox, leading, item.name().to_owned(), item.description(now), components::row(components::SPACE_SM).child(restore).child(delete), selected, theme)
    }

    fn render_group(&self, index: usize, items: &[&TrashItem], now: i64, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::Div {
        let keys: Vec<TrashKey> = items.iter().map(|item| item.key()).collect();
        let all_selected = keys.iter().all(|key| self.selection.contains(key));
        let page = cx.entity();
        let toggle = components::browser_settings_header_button(format!("trash-select-group-{index}"), if all_selected { "Clear selection" } else { "Select all" }, false, theme).on_click(move |_, _, cx| {
            page.update(cx, |page, cx| {
                for key in &keys {
                    if all_selected {
                        page.selection.remove(key);
                    } else {
                        page.selection.insert(*key);
                    }
                }
                cx.notify();
            })
        });
        components::browser_settings_fieldset_with_actions(GROUPS[index].0, toggle, theme).children(items.iter().map(|item| self.render_item(item, now, theme, cx)))
    }

    fn render_selection_bar(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let selected = self.selected_items();
        if selected.is_empty() {
            return None;
        }
        let page = cx.entity();
        let clear = components::link_button("trash-clear-selection", "Clear", theme, false).on_click({
            let page = page.clone();
            move |_, _, cx| {
                page.update(cx, |page, cx| {
                    page.selection.clear();
                    cx.notify();
                })
            }
        });
        let restore = components::topbar_action_button("trash-restore-selection", "Restore", IconName::Undo2, theme).on_click({
            let page = page.clone();
            let selected = selected.clone();
            move |_, _, cx| page.update(cx, |page, cx| page.restore(selected.clone(), cx))
        });
        let delete = components::topbar_danger_button("trash-delete-selection", "Delete permanently", IconName::Delete, theme).on_click({
            let selected = selected.clone();
            move |_, window, cx| page.update(cx, |page, cx| page.confirm_purge(selected.clone(), false, window, cx))
        });
        Some(components::selection_bar(format!("{} selected", count_label(&selected)), components::row(components::SPACE_SM).child(clear).child(restore).child(delete), theme))
    }
}

impl Render for TrashPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.covers_need_reload {
            self.covers_need_reload = false;
            self.load_covers(cx);
        }
        let theme = components::browser_theme(cx);
        let items = self.contents.as_ref().map(trash_items).unwrap_or_default();
        let page = cx.entity();
        let heading = if components::uses_mobile_navigation(window) {
            components::mobile_navigation_heading("Trash", crate::navigation::mobile_back(true, theme), theme)
        } else {
            components::browser_topbar_left().child(div().text_xl().child("Trash"))
        };
        let search = (!items.is_empty()).then(|| components::browser_search_control(&self.search_input(window, cx)));
        let empty_trash = (!items.is_empty()).then(|| {
            let everything = items.clone();
            components::topbar_danger_button("empty-trash", "Empty Trash", IconName::Delete, theme).on_click(move |_, window, cx| {
                page.update(cx, |page, cx| page.confirm_purge(everything.clone(), true, window, cx));
            })
        });
        let header = components::topbar(theme).child(heading).children(search).child(components::browser_topbar_right().children(empty_trash));

        let mut body = components::browser_settings_page();
        match self.contents.as_ref() {
            None => body = body.child(components::browser_loading_message("Loading Trash…", theme)),
            Some(_) if items.is_empty() => body = body.child(components::browser_actionable_empty_without_description("Trash is empty", div(), theme)),
            Some(_) => {
                let now = now_ms();
                let shown: Vec<&TrashItem> = items.iter().filter(|item| self.query.is_empty() || item.matches(&self.query)).collect();
                let mut content = components::browser_settings_content();
                if shown.is_empty() {
                    content = content.child(components::browser_inline_note("No matches", theme));
                }
                for index in 0..GROUPS.len() {
                    let group: Vec<&TrashItem> = shown.iter().copied().filter(|item| group_of(item.deleted_at(), now) == index).collect();
                    if !group.is_empty() {
                        content = content.child(self.render_group(index, &group, now, theme, cx));
                    }
                }
                body = body.child(content);
            }
        }
        let page = cx.entity();
        let body = with_retry_banner(body.into_any_element(), self.error.clone(), "retry-section", theme, move |_, _, cx| page.update(cx, |page, cx| page.refresh(cx)));
        components::library_section_content().track_focus(&self.focus).child(header).child(body).children(self.render_selection_bar(theme, cx)).into_any_element()
    }
}

impl Focusable for TrashPage {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
