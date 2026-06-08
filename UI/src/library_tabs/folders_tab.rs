use crate::components::{create_resizable_hsplit, create_tab_button};
use crate::components::library_views::{
    books_grid, books_grid_with_empty_state, breadcrumb_part, breadcrumb_root, breadcrumb_separator, dashboard_folder_list, dashboard_folder_row,
    dashboard_folder_section, folder_layout_bar, folder_tree_panel,
};
use crate::page_navigation::{Page, ReaderState, Signals};
use crate::style::{create_book_title_label, style_book_cover};
use crate::toc::{toc_view_with_collapse_state_active_link, CollapseState, TocEntry};
use epub_svelte_lib::library::{Book, Dir, DirContent, Library};
use floem::event::EventPropagation;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::prelude::{container, create_rw_signal, dyn_view, h_stack, label, v_stack, Decorators, RwSignal, ScrollExt, SignalGet, SignalUpdate, Stack};
use floem::style::AlignItems;
use floem::views::{img, stack_from_iter};
use floem::View;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Copy, Eq, PartialEq)]
enum FolderViewMode {
    SplitPane,
    Dashboard,
}

pub fn folders_tab_view(library_id: String, library_name: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals) -> impl View {
    let folder_view_mode = create_rw_signal(FolderViewMode::SplitPane);
    let library_id_for_content = library_id.clone();
    let library_name_for_content = library_name.clone();
    let content = dyn_view(move || match folder_view_mode.get() {
        FolderViewMode::SplitPane => Box::new(split_pane_view(library_id_for_content.clone(), library_name_for_content.clone(), folder_stack, signals.clone()).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0))) as Box<dyn View>,
        FolderViewMode::Dashboard => Box::new(dashboard_view(library_id_for_content.clone(), library_name_for_content.clone(), folder_stack, signals.clone()).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0))) as Box<dyn View>,
    })
    .style(|s| s.width_full().flex_grow(1.0).flex_basis(0).min_height(0).min_width(0));

    let settings = create_folder_layout_settings(folder_view_mode);

    v_stack((settings, content))
        .on_key_down(
            Key::Named(NamedKey::Backspace),
            |_mods: Modifiers| true,
            move |_| {
                folder_stack.update(|stack| {
                    if !stack.is_empty() {
                        stack.pop();
                    }
                });
            },
        )
        .style(|s| s.width_full().height_full().flex_col().flex_grow(1.0).flex_basis(0).min_height(0).min_width(0))
}

fn create_folder_layout_settings(folder_view_mode: RwSignal<FolderViewMode>) -> impl View {
    let split = create_tab_button("Split", move || folder_view_mode.get() == FolderViewMode::SplitPane).on_click(move |_| {
            folder_view_mode.set(FolderViewMode::SplitPane);
            EventPropagation::Continue
        });
    let dashboard = create_tab_button("Dashboard", move || folder_view_mode.get() == FolderViewMode::Dashboard).on_click(move |_| {
            folder_view_mode.set(FolderViewMode::Dashboard);
            EventPropagation::Continue
        });
    folder_layout_bar(split, dashboard)
}

fn split_pane_view(
    library_id: String,
    library_name: String,
    folder_stack: RwSignal<Vec<(String, String)>>,
    signals: Signals,
) -> impl View {
    let collapse_state: CollapseState = create_rw_signal(HashMap::new());
    let left_sidebar_width = create_rw_signal(280.0);

    let tree_panel = {
        let library_id_for_tree = library_id.clone();
        let signals_for_tree = signals.clone();
        dyn_view(move || {
            let _refresh_tick = signals_for_tree.library_refresh_tick.get();
            let tree_entries = build_folder_toc_entries(&signals_for_tree, &library_id_for_tree);
            let root_entry = TocEntry {
                title: library_name.clone(),
                link: "__root__".to_string(),
                resolved: None,
                children: tree_entries,
            };
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
            let tree_view = if tree_entries.is_empty() { container(label(|| "No folders")).style(|s| s.padding_horiz(10).padding_vert(8)) } else { container(toc_view_with_collapse_state_active_link(tree_entries, on_folder_click, active_folder_link, 0, collapse_state.clone())) };
            Box::new(folder_tree_panel(tree_view.scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0)))) as Box<dyn View>
        })
        .style(|s| s.height_full().min_height(0))
    };

    let books_panel = {
        let library_id_for_books = library_id;
        let signals_for_books = signals;
        dyn_view(move || {
            let _refresh_tick = signals_for_books.library_refresh_tick.get();
            let folder_id = folder_stack.get().last().map(|(id, _)| id.clone()).unwrap_or_else(|| "__root__".to_string());
            let dir_content = signals_for_books
                .library_by_id(&library_id_for_books)
                .map(|s| s.handle(signals_for_books.library_event_counter.clone()).get_dir_contents(&folder_id))
                .unwrap_or(DirContent { books: vec![], dirs: vec![] });

            let books_scroll = create_books_grid(dir_content.books, library_id_for_books.clone(), signals_for_books.clone())
                .scroll()
                .style(|s| s.size_full().min_height(0).min_width(0));
            Box::new(container(books_scroll).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).min_width(0))) as Box<dyn View>
        })
        .style(|s| s.width_full().height_full().flex_grow(1.0).flex_basis(0).min_height(0).min_width(0))
    };

    create_resizable_hsplit(tree_panel, books_panel, left_sidebar_width, 220.0, 560.0).style(|s| s.width_full().height_full().flex_grow(1.0).flex_basis(0).min_height(0).min_width(0).padding(16))
}

fn dashboard_view(library_id: String, library_name: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals) -> impl View {
    dyn_view(move || {
        let _refresh_tick = signals.library_refresh_tick.get();
        let folder_id = folder_stack.get().last().map(|(id, _)| id.clone()).unwrap_or_else(|| "__root__".to_string());
        let dir_content = signals
            .library_by_id(&library_id)
            .map(|s| s.handle(signals.library_event_counter.clone()).get_dir_contents(&folder_id))
            .unwrap_or(DirContent { books: vec![], dirs: vec![] });

        Box::new(
            container(v_stack((
                create_folder_breadcrumb(folder_stack, library_name.clone()),
                render_folder_dashboard(dir_content, library_id.clone(), folder_stack, signals.clone()),
            )))
                .style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).padding(16)),
        ) as Box<dyn View>
    })
}

fn build_folder_toc_entries(signals: &Signals, library_id: &str) -> Vec<TocEntry> {
    let Some(state) = signals.library_by_id(library_id) else {
        return vec![];
    };
    let library = state.handle(signals.library_event_counter.clone());
    let root_dirs = library
        .get_dir_contents("__root__")
        .dirs
        .into_iter()
        .filter(|dir| dir.id != "__root__")
        .collect::<Vec<_>>();
    let mut ancestry = HashSet::new();
    ancestry.insert("__root__".to_string());
    root_dirs.into_iter().map(|dir| build_folder_toc_node(&library, dir, &mut ancestry)).collect()
}

fn build_folder_toc_node(library: &Library, dir: Dir, ancestry: &mut HashSet<String>) -> TocEntry {
    let dir_id = dir.id;
    let dir_name = dir.name;

    let children = if ancestry.insert(dir_id.clone()) {
        let built = library
            .get_dir_contents(&dir_id)
            .dirs
            .into_iter()
            .filter(|child| child.id != "__root__")
            .map(|child| build_folder_toc_node(library, child, ancestry))
            .collect();
        ancestry.remove(&dir_id);
        built
    } else {
        vec![]
    };

    TocEntry { title: dir_name, link: dir_id, resolved: None, children }
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

fn render_folder_dashboard(dir_content: DirContent, library_id: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals) -> impl View {
    let dashboard_dirs: Vec<Dir> = dir_content.dirs.into_iter().filter(|dir| dir.id != "__root__").collect();
    let has_folders = !dashboard_dirs.is_empty();
    let library_id_for_counts = library_id.clone();
    let signals_for_counts = signals.clone();
    let folder_list = dashboard_folder_list(dashboard_dirs.into_iter().map(move |dir| {
        let count = get_book_count_for_folder(&library_id_for_counts, &dir.id, &signals_for_counts);
        create_dashboard_folder_row(dir, count, folder_stack)
    }));

    let folders_section = dashboard_folder_section(has_folders, folder_list);

    let books_section = container(create_books_grid(dir_content.books, library_id, signals).scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0)))
        .style(|s| s.width_full().height_full().min_height(0));

    h_stack((books_section, folders_section)).style(move |s| {
        let base = s.width_full().height_full().flex_grow(1.0).min_height(0);
        if has_folders {
            base.gap(16)
        } else {
            base
        }
    })
}

pub fn create_books_grid(books: Vec<Book>, library_id: String, signals: Signals) -> impl View {
    let book_count = books.len();
    let grid = books_grid(books.into_iter().map(move |book| create_book_cover(book, library_id.clone(), signals.clone())));

    books_grid_with_empty_state(book_count, grid)
}

fn get_book_count_for_folder(library_id: &str, folder_id: &str, signals: &Signals) -> usize {
    signals
        .library_by_id(library_id)
        .map(|s| s.handle(signals.library_event_counter.clone()).get_dir_contents(folder_id).books.len())
        .unwrap_or(0)
}

fn create_dashboard_folder_row(dir: Dir, book_count: usize, folder_stack: RwSignal<Vec<(String, String)>>) -> impl View {
    let dir_id = dir.id;
    let dir_name = dir.name;
    let dir_name_for_push = dir_name.clone();

    dashboard_folder_row(dir_name, book_count)
        .on_click(move |_| {
            folder_stack.update(|stack| stack.push((dir_id.clone(), dir_name_for_push.clone())));
            EventPropagation::Continue
        })
}

pub fn create_book_cover(book: Book, library_id: String, signals: Signals) -> Stack {
    let title_label = create_book_title_label(book.title.clone());
    let cover = match book.cover {
        None => container(label(move || "Placeholder".to_string())),
        Some(book_cover) => {
            let cover_image = img(move || book_cover.clone()).on_click(move |_| {
                let reader_state = ReaderState { book_id: book.id.clone(), library_id: library_id.clone(), book_title: book.title.clone() };
                signals.navigate(Page::Reader(reader_state));
                EventPropagation::Continue
            });
            let cover_image = style_book_cover(cover_image);
            container(cover_image)
        }
    };
    v_stack((cover, title_label))
}

fn create_folder_breadcrumb(folder_stack: RwSignal<Vec<(String, String)>>, library_name: String) -> impl View {
    const MAX_BREADCRUMB_DEPTH: usize = 32;
    let root_crumb = breadcrumb_root(library_name)
        .on_click(move |_| {
            folder_stack.set(vec![]);
            EventPropagation::Continue
        });

    let crumb_slots = stack_from_iter((0..MAX_BREADCRUMB_DEPTH).map(move |idx| {
        h_stack((
            breadcrumb_separator(),
            breadcrumb_part(move || {
                let crumbs = folder_stack.get();
                if idx < crumbs.len() {
                    crumbs[idx].1.clone()
                } else {
                    String::new()
                }
            })
            .on_click(move |_| {
                let len = folder_stack.get_untracked().len();
                if idx < len {
                    folder_stack.update(|stack| {
                        stack.truncate(idx + 1);
                    });
                }
                EventPropagation::Continue
            }),
        ))
        .style(move |s| {
            let len = folder_stack.get();
            if idx < len.len() {
                s.align_items(AlignItems::Center)
            } else {
                s.hide()
            }
        })
    }))
    .style(|s| s.flex_row().align_items(AlignItems::Center));

    container(h_stack((root_crumb, crumb_slots))).style(|s| s.width_full().align_items(AlignItems::Center).justify_center().padding_vert(4))
}
