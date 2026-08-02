use crate::components::{
    books_scroll, centered_modal, create_overlay_scrim, danger_text, dynamic_button, empty_view, folder_books_panel, folder_books_slot, folder_breadcrumb_bar, folder_breadcrumb_crumb, folder_breadcrumb_part, folder_breadcrumb_root,
    folder_breadcrumb_slots, folder_tree_panel, folder_tree_scroll, folder_tree_slot, folder_workspace, form_actions, form_field_row, inline_message, modal_panel, modal_title, page_surface, resizable_split, secondary_button,
    toc_view_with_collapse_state_active_link, CollapseState, FixedSide, TocEntry,
};
use crate::library_tabs::create_books_grid;
use crate::page_navigation::Signals;
use epub_svelte_lib::app_core::AppCore;
use epub_svelte_lib::library::{Dir, DirContent};
use floem::event::EventPropagation;
use floem::prelude::{container, create_rw_signal, dyn_view, h_stack, text_input, Decorators, RwSignal, SignalGet, SignalUpdate, View};
use floem::views::stack;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub fn folders_tab_view(library_id: String, library_name: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals) -> impl View {
    page_surface(folder_split_view(library_id, library_name, folder_stack, signals))
}

fn folder_split_view(library_id: String, library_name: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals) -> impl View {
    let collapse_state: CollapseState = create_rw_signal(HashMap::new());
    let left_sidebar_width = create_rw_signal(280.0);
    let show_create_folder = create_rw_signal(false);
    let new_folder_name = create_rw_signal(String::new());
    let create_folder_error = create_rw_signal(None::<String>);
    let tree_library_name = library_name.clone();
    let content_library_name = library_name.clone();

    let tree_panel = {
        let library_id_for_tree = library_id.clone();
        let signals_for_tree = signals.clone();
        folder_tree_slot(dyn_view(move || {
            let _refresh_tick = signals_for_tree.library_refresh_tick.get();
            let tree_entries = build_folder_toc_entries(&signals_for_tree, &library_id_for_tree);
            let root_entry = TocEntry { title: tree_library_name.clone(), link: "__root__".to_string(), resolved: None, children: tree_entries };
            let tree_entries = vec![root_entry];
            let active_folder_link = create_rw_signal(Some(folder_stack.get().last().map(|(id, _)| id.clone()).unwrap_or_else(|| "__root__".to_string())));
            let tree_entries_for_click = Rc::new(tree_entries.clone());
            let on_folder_click: Rc<dyn Fn(String)> = Rc::new(move |clicked_id: String| {
                let mut path = vec![];
                if clicked_id != "__root__" && !find_folder_path(tree_entries_for_click.as_slice(), &clicked_id, &mut path) {
                    return;
                }
                folder_stack.set(path);
            });
            let tree_view = if tree_entries.is_empty() { container(inline_message("No folders")) } else { container(toc_view_with_collapse_state_active_link(tree_entries, on_folder_click, active_folder_link, 0, collapse_state)) };
            Box::new(folder_tree_panel(folder_tree_scroll(tree_view))) as Box<dyn View>
        }))
    };

    let books_panel = {
        let library_id_for_books = library_id.clone();
        let signals_for_books = signals.clone();
        folder_books_slot(dyn_view(move || {
            let _refresh_tick = signals_for_books.library_refresh_tick.get();
            let folder_id = folder_stack.get().last().map(|(id, _)| id.clone()).unwrap_or_else(|| "__root__".to_string());
            let dir_content = signals_for_books.app_core.directory_in(&library_id_for_books, &folder_id).unwrap_or_else(|error| {
                eprintln!("failed to load folder {folder_id}: {error}");
                DirContent { books: vec![], dirs: vec![] }
            });

            let breadcrumb = create_folder_breadcrumb(folder_stack, content_library_name.clone());
            let new_folder_button = secondary_button("New folder").on_click(move |_| {
                new_folder_name.set(String::new());
                create_folder_error.set(None);
                show_create_folder.set(true);
                EventPropagation::Stop
            });
            let header = h_stack((breadcrumb, new_folder_button)).style(|style| style.width_full().items_center().justify_between());
            let grid_scroll = books_scroll(create_books_grid(dir_content.books, library_id_for_books.clone(), signals_for_books.clone()));
            Box::new(folder_books_panel(header, grid_scroll)) as Box<dyn View>
        }))
    };

    let workspace = folder_workspace(resizable_split(FixedSide::Left, tree_panel, books_panel, left_sidebar_width, 120.0, 560.0));
    let create_folder = create_folder_modal(library_id, folder_stack, signals, show_create_folder, new_folder_name, create_folder_error);
    stack((workspace, create_folder))
}

fn create_folder_modal(library_id: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals, show_modal: RwSignal<bool>, name: RwSignal<String>, error: RwSignal<Option<String>>) -> impl View {
    let signals_for_submit = signals.clone();
    let submit = dynamic_button(|| "Create".to_string()).on_click(move |_| {
        error.set(None);
        let parent_id = folder_stack.get_untracked().last().map(|(id, _)| id.clone()).unwrap_or_else(|| "__root__".to_string());
        if signals_for_submit.library_by_id(&library_id).is_none() {
            error.set(Some("Library is no longer available".to_string()));
            return EventPropagation::Stop;
        }
        match signals_for_submit.app_core.create_directory_in(&library_id, &parent_id, &name.get_untracked()) {
            Ok(created) => {
                folder_stack.update(|path| path.push((created.id().as_str().to_owned(), created.name().as_str().to_owned())));
                name.set(String::new());
                show_modal.set(false);
                signals_for_submit.library_refresh_tick.update(|tick| *tick = tick.saturating_add(1));
            }
            Err(create_error) => error.set(Some(create_error.to_string())),
        }
        EventPropagation::Stop
    });

    let cancel = secondary_button("Cancel").on_click(move |_| {
        error.set(None);
        show_modal.set(false);
        EventPropagation::Stop
    });
    let error_label = dyn_view(move || -> Box<dyn View> {
        match error.get() {
            Some(message) => Box::new(danger_text(message)),
            None => Box::new(empty_view()),
        }
    });
    let panel = modal_panel((modal_title("Create folder"), form_field_row("Folder name", text_input(name)), error_label, form_actions(submit, cancel))).on_click(|_| EventPropagation::Stop);
    create_overlay_scrim(
        show_modal,
        centered_modal(panel).on_click(move |_| {
            show_modal.set(false);
            EventPropagation::Stop
        }),
    )
}

fn build_folder_toc_entries(signals: &Signals, library_id: &str) -> Vec<TocEntry> {
    if signals.library_by_id(library_id).is_none() {
        return vec![];
    }
    let root_dirs = signals
        .app_core
        .directory_in(library_id, "__root__")
        .unwrap_or_else(|error| {
            eprintln!("failed to load root folders: {error}");
            DirContent { books: vec![], dirs: vec![] }
        })
        .dirs
        .into_iter()
        .filter(|dir| dir.id().as_str() != "__root__")
        .collect::<Vec<_>>();
    let mut ancestry = HashSet::new();
    ancestry.insert("__root__".to_string());
    root_dirs.into_iter().map(|dir| build_folder_toc_node(&signals.app_core, library_id, dir, &mut ancestry)).collect()
}

fn build_folder_toc_node(core: &AppCore, library_id: &str, dir: Dir, ancestry: &mut HashSet<String>) -> TocEntry {
    let dir_link = dir.id().clone().into_string();
    let dir_name = dir.name().as_str().to_owned();
    let children = if ancestry.insert(dir_link.clone()) {
        let built = core
            .directory_in(library_id, &dir_link)
            .unwrap_or_else(|error| {
                eprintln!("failed to load folder {dir_link}: {error}");
                DirContent { books: vec![], dirs: vec![] }
            })
            .dirs
            .into_iter()
            .filter(|child| child.id().as_str() != "__root__")
            .map(|child| build_folder_toc_node(core, library_id, child, ancestry))
            .collect();
        ancestry.remove(&dir_link);
        built
    } else {
        vec![]
    };

    TocEntry { title: dir_name, link: dir_link, resolved: None, children }
}

fn find_folder_path(entries: &[TocEntry], target: &str, path: &mut Vec<(String, String)>) -> bool {
    for entry in entries {
        if entry.link == target {
            if entry.link != "__root__" {
                path.push((entry.link.clone(), entry.title.clone()));
            }
            return true;
        }

        if entry.link != "__root__" {
            path.push((entry.link.clone(), entry.title.clone()));
        }
        if find_folder_path(&entry.children, target, path) {
            return true;
        }
        if entry.link != "__root__" {
            path.pop();
        }
    }
    false
}

fn create_folder_breadcrumb(folder_stack: RwSignal<Vec<(String, String)>>, library_name: String) -> impl View {
    const MAX_BREADCRUMB_DEPTH: usize = 32;
    let root_crumb = folder_breadcrumb_root(library_name, move |_| {
        folder_stack.set(vec![]);
        EventPropagation::Continue
    });

    let crumb_slots = folder_breadcrumb_slots((0..MAX_BREADCRUMB_DEPTH).map(move |idx| {
        let part = folder_breadcrumb_part(
            move || {
                let crumbs = folder_stack.get();
                if idx < crumbs.len() {
                    crumbs[idx].1.clone()
                } else {
                    String::new()
                }
            },
            move |_| {
                let len = folder_stack.get_untracked().len();
                if idx < len {
                    folder_stack.update(|stack| {
                        stack.truncate(idx + 1);
                    });
                }
                EventPropagation::Continue
            },
        );
        folder_breadcrumb_crumb(part, move || folder_stack.get().len() > idx)
    }));

    folder_breadcrumb_bar(root_crumb, crumb_slots)
}
