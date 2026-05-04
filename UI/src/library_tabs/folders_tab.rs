use crate::components::create_tab_button;
use crate::page_navigation::{Page, ReaderState, Signals};
use crate::style::{create_book_title_label, style_book_cover, ACTIVE_HIGHLIGHT, BORDER_COLOR, PANEL_BG};
use crate::toc::{toc_view_with_collapse_state, CollapseState, TocEntry};
use epub_svelte_lib::library::{Book, Dir, DirContent, Library};
use floem::event::EventPropagation;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::peniko::Color;
use floem::prelude::{button, container, create_rw_signal, dyn_view, h_stack, label, v_stack, Decorators, RwSignal, ScrollExt, SignalGet, SignalUpdate, Stack};
use floem::style::{AlignItems, FlexWrap};
use floem::views::{img, stack, stack_from_iter};
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
    let split_content =
        split_pane_view(library_id.clone(), library_name.clone(), folder_stack, signals.clone()).style(move |s| if folder_view_mode.get() == FolderViewMode::SplitPane { s.width_full().height_full().flex_grow(1.0).min_height(0) } else { s.hide() });
    let dashboard_content = dashboard_view(library_id, folder_stack, signals).style(move |s| if folder_view_mode.get() == FolderViewMode::Dashboard { s.width_full().height_full().flex_grow(1.0).min_height(0) } else { s.hide() });
    let content = stack((split_content, dashboard_content)).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0));

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
        .style(|s| s.width_full().height_full().background(Color::WHITE).min_height(0))
}

fn create_folder_layout_settings(folder_view_mode: RwSignal<FolderViewMode>) -> impl View {
    h_stack((
        label(|| "Layout").style(|s| s.font_size(14).color(Color::rgb8(55, 65, 81)).padding_right(6)),
        create_tab_button("Split", move || folder_view_mode.get() == FolderViewMode::SplitPane).on_click(move |_| {
            folder_view_mode.set(FolderViewMode::SplitPane);
            EventPropagation::Continue
        }),
        create_tab_button("Dashboard", move || folder_view_mode.get() == FolderViewMode::Dashboard).on_click(move |_| {
            folder_view_mode.set(FolderViewMode::Dashboard);
            EventPropagation::Continue
        }),
    ))
    .style(|s| s.width_full().padding_horiz(16).padding_top(10).padding_bottom(6).align_items(AlignItems::Center).gap(8).border_bottom(1).border_color(BORDER_COLOR))
}

fn split_pane_view(library_id: String, library_name: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals) -> impl View {
    let collapse_state: CollapseState = create_rw_signal(HashMap::new());

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
            let active_entry = create_rw_signal(None::<(usize, Option<String>)>);
            let tree_entries_for_click = Rc::new(tree_entries.clone());
            let on_folder_click: Rc<dyn Fn(String)> = Rc::new(move |clicked_id: String| {
                let mut path = vec![];
                if clicked_id != "__root__" && !find_folder_path(tree_entries_for_click.as_slice(), &clicked_id, &mut path) {
                    return;
                }
                folder_stack.set(path);
            });
            let tree_view = if tree_entries.is_empty() { container(label(|| "No folders")).style(|s| s.padding_horiz(10).padding_vert(8)) } else { container(toc_view_with_collapse_state(tree_entries, on_folder_click, active_entry, 0, collapse_state.clone())) };
            Box::new(
                v_stack((tree_view.scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0)),))
                    .style(|s| s.width(280).height_full().padding(12).background(PANEL_BG).border(1).border_color(BORDER_COLOR).border_radius(12).min_height(0)),
            ) as Box<dyn View>
        })
    };

    let books_panel = {
        let library_id_for_books = library_id;
        let signals_for_books = signals;
        dyn_view(move || {
            let _refresh_tick = signals_for_books.library_refresh_tick.get();
            let folder_id = folder_stack.get().last().map(|(id, _)| id.clone()).unwrap_or_else(|| "__root__".to_string());
            let handles = signals_for_books.library_handles.get_untracked();
            let dir_content = handles.get(&library_id_for_books).map(|library| library.get_dir_contents(&folder_id)).unwrap_or(DirContent { books: vec![], dirs: vec![] });

            Box::new(
                v_stack((create_books_grid(dir_content.books, library_id_for_books.clone(), signals_for_books.clone()).scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0)),))
                    .style(|s| s.width_full().height_full().min_height(0)),
            ) as Box<dyn View>
        })
    };

    h_stack((tree_panel, books_panel)).style(|s| s.width_full().height_full().gap(14).min_height(0).padding(16))
}

fn dashboard_view(library_id: String, folder_stack: RwSignal<Vec<(String, String)>>, signals: Signals) -> impl View {
    dyn_view(move || {
        let _refresh_tick = signals.library_refresh_tick.get();
        let folder_id = folder_stack.get().last().map(|(id, _)| id.clone()).unwrap_or_else(|| "__root__".to_string());
        let handles = signals.library_handles.get_untracked();
        let dir_content = handles.get(&library_id).map(|library| library.get_dir_contents(&folder_id)).unwrap_or(DirContent { books: vec![], dirs: vec![] });

        Box::new(
            container(v_stack((
                create_folder_breadcrumb(folder_stack, signals.active_library_name),
                render_folder_dashboard(dir_content, library_id.clone(), folder_stack, signals.clone()),
            )))
                .style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).padding(16)),
        ) as Box<dyn View>
    })
}

fn build_folder_toc_entries(signals: &Signals, library_id: &str) -> Vec<TocEntry> {
    let handles = signals.library_handles.get_untracked();
    let Some(library) = handles.get(library_id) else {
        return vec![];
    };

    let root_dirs = library.get_dir_contents("__root__").dirs;
    let mut ancestry = HashSet::new();
    ancestry.insert("__root__".to_string());
    root_dirs.into_iter().map(|dir| build_folder_toc_node(library, dir, &mut ancestry)).collect()
}

fn build_folder_toc_node(library: &Library, dir: Dir, ancestry: &mut HashSet<String>) -> TocEntry {
    let dir_id = dir.id;
    let dir_name = dir.name;

    let children = if ancestry.insert(dir_id.clone()) {
        let built = library.get_dir_contents(&dir_id).dirs.into_iter().map(|child| build_folder_toc_node(library, child, ancestry)).collect();
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
    let library_id_for_counts = library_id.clone();
    let signals_for_counts = signals.clone();
    let folder_cards = stack_from_iter(dir_content.dirs.into_iter().map(move |dir| {
        let count = get_book_count_for_folder(&library_id_for_counts, &dir.id, &signals_for_counts);
        create_dashboard_folder_card(dir, count, folder_stack)
    }))
    .style(|s| s.width_full().gap(12).flex_row().flex_wrap(FlexWrap::Wrap));

    let folders_section = container(folder_cards).style(|s| s.width_full().padding(12).background(PANEL_BG).border(1).border_color(BORDER_COLOR).border_radius(12));

    let books_section = create_books_grid(dir_content.books, library_id, signals);

    v_stack((folders_section, books_section)).style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0).gap(18)).scroll().style(|s| s.width_full().height_full().flex_grow(1.0).min_height(0))
}

pub fn create_books_grid(books: Vec<Book>, library_id: String, signals: Signals) -> impl View {
    let book_count = books.len();
    let books_grid = stack_from_iter(books.into_iter().map(move |book| create_book_cover(book, library_id.clone(), signals.clone()))).style(|s| s.gap(28).flex_row().flex_wrap(FlexWrap::Wrap).width_full());

    v_stack((label(move || "No books in this folder".to_string()).style(move |s| if book_count == 0 { s.padding_horiz(6).padding_vert(4).color(Color::rgb8(75, 85, 99)) } else { s.hide() }), books_grid)).style(|s| s.width_full().gap(10))
}

fn get_book_count_for_folder(library_id: &str, folder_id: &str, signals: &Signals) -> usize {
    let handles = signals.library_handles.get_untracked();
    handles.get(library_id).map(|library| library.get_dir_contents(folder_id).books.len()).unwrap_or(0)
}

fn create_dashboard_folder_card(dir: Dir, book_count: usize, folder_stack: RwSignal<Vec<(String, String)>>) -> impl View {
    let dir_id = dir.id;
    let dir_name = dir.name;
    let dir_name_for_push = dir_name.clone();

    button(v_stack((label(move || format!("📁 {dir_name}")).style(|s| s.font_size(15).font_bold().text_ellipsis().width(220)), label(move || format!("{book_count} books")).style(|s| s.font_size(12).color(Color::rgb8(75, 85, 99))))))
        .on_click(move |_| {
            folder_stack.update(|stack| stack.push((dir_id.clone(), dir_name_for_push.clone())));
            EventPropagation::Continue
        })
        .style(|s| s.width(240).padding_horiz(12).padding_vert(10).justify_start().align_items(AlignItems::Start).border(1).border_color(BORDER_COLOR).border_radius(10).background(Color::WHITE).hover(|s| s.background(ACTIVE_HIGHLIGHT)))
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

fn create_folder_breadcrumb(folder_stack: RwSignal<Vec<(String, String)>>, library_name: RwSignal<String>) -> impl View {
    const MAX_BREADCRUMB_DEPTH: usize = 32;
    let root_crumb = label(move || library_name.get())
        .on_click(move |_| {
            folder_stack.set(vec![]);
            EventPropagation::Continue
        })
        .style(|s| s.padding_horiz(6).padding_vert(4).font_bold().hover(|s| s.background(ACTIVE_HIGHLIGHT)));

    let crumb_slots = stack_from_iter((0..MAX_BREADCRUMB_DEPTH).map(move |idx| {
        h_stack((
            label(|| "/").style(|s| s.padding_left(2).padding_right(2)),
            label(move || {
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
            })
            .style(|s| s.padding_horiz(6).padding_vert(4).hover(|s| s.background(ACTIVE_HIGHLIGHT))),
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
