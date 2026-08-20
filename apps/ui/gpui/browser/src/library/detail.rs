use std::collections::HashSet;
use std::rc::Rc;
use std::vec;

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, IntoElement, Render, SharedString, Task, TextRun, Window, div, font, px};
use gpui_component::progress::Progress;
use gpui_component::{ElementExt as _, Sizable, Size};
use html_view_core::RendererInitialConfig;
use html_view_gpui::HtmlView;
use ui_components as components;

use crate::description::description_provider;
use crate::stores::interface_font;

use super::services::LibraryContext;
use super::widgets::{BookGrid, LoadedCover, loaded_cover_from_jpeg};

const DESCRIPTION_FONT_SIZE: f32 = 16.0;
const DESCRIPTION_CHARACTER_COLUMNS: f32 = 80.0;
const DETAIL_HERO_GAP: f32 = 24.0;

fn description_column_width(window: &Window, font_family: SharedString) -> f32 {
    let sample = "0";
    let run = TextRun { len: sample.len(), font: font(font_family), ..Default::default() };
    f32::from(window.text_system().layout_line(sample, px(DESCRIPTION_FONT_SIZE), &[run], None).width) * DESCRIPTION_CHARACTER_COLUMNS
}

pub(crate) struct BookDetailPage {
    ui: LibraryContext,
    content_hash: epub_svelte_lib::ContentHash,
    detail: Option<epub_svelte_lib::library::BookDetail>,
    related: BookGrid,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    toc_task: Option<Task<()>>,
    cover: Option<LoadedCover>,
    description: Option<Entity<HtmlView>>,
    description_font: Option<SharedString>,
    toc: Vec<app_contract::library::BookTocEntry>,
    toc_loading: bool,
    toc_error: Option<SharedString>,
    toc_open: bool,
    expanded_toc: HashSet<String>,
    compact_layout: bool,
    on_back: Rc<dyn Fn(&mut gpui::App)>,
}

impl BookDetailPage {
    pub(crate) fn new(ui: LibraryContext, content_hash: epub_svelte_lib::ContentHash, on_back: Rc<dyn Fn(&mut gpui::App)>, cx: &mut Context<Self>) -> Self {
        let related = BookGrid::new(ui.clone(), false);
        let mut page = Self {
            ui,
            content_hash,
            detail: None,
            related,
            error: None,
            task: None,
            toc_task: None,
            cover: None,
            description: None,
            description_font: None,
            toc: Vec::new(),
            toc_loading: true,
            toc_error: None,
            toc_open: false,
            expanded_toc: HashSet::new(),
            compact_layout: true,
            on_back,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let load = self.ui.load_book_detail(self.content_hash.clone());
        let cover = self.ui.load_cover(self.content_hash.clone());
        let toc = self.ui.load_book_toc(self.content_hash.clone());
        self.task = Some(cx.spawn(async move |page, cx| {
            let (result, cover) = futures_util::join!(load, cover);
            let _ = page.update(cx, |page, cx| match result {
                Ok(detail) => {
                    page.cover = cover.ok().flatten().and_then(loaded_cover_from_jpeg);
                    page.related.set_books(detail.related.clone(), cx);
                    page.detail = Some(detail);
                    page.error = None;
                    cx.notify();
                }
                Err(error) => {
                    page.error = Some(error.into());
                    cx.notify();
                }
            });
        }));
        self.toc_task = Some(cx.spawn(async move |page, cx| {
            let result = toc.await;
            let _ = page.update(cx, |page, cx| {
                page.toc_loading = false;
                match result {
                    Ok(toc) => {
                        page.toc = toc;
                        page.toc_error = None;
                    }
                    Err(error) => page.toc_error = Some(error.into()),
                }
                cx.notify();
            });
        }));
    }

    fn toggle_toc(&mut self, cx: &mut Context<Self>) {
        self.toc_open = !self.toc_open;
        cx.notify();
    }

    fn toggle_toc_branch(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded_toc.remove(&key) {
            self.expanded_toc.insert(key);
        }
        cx.notify();
    }
}

fn toc_rows(
    entries: &[app_contract::library::BookTocEntry], depth: usize, parent_key: &str, expanded: &HashSet<String>, page: &gpui::WeakEntity<BookDetailPage>, ui: &LibraryContext, content_hash: &epub_svelte_lib::ContentHash, book_title: &str,
    theme: components::BrowserTheme,
) -> Vec<gpui::AnyElement> {
    let mut rows = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let key = if parent_key.is_empty() { index.to_string() } else { format!("{parent_key}.{index}") };
        let is_expanded = expanded.contains(&key);
        let label = entry.title.clone();
        let target = entry.target.clone();
        let row_id = SharedString::from(format!("book-detail-toc-{key}"));
        let open_ui = ui.clone();
        let open_hash = content_hash.clone();
        let open_title = book_title.to_owned();
        let disclosure = if entry.children.is_empty() {
            div().w(px(32.0)).flex_none().into_any_element()
        } else {
            let toggle_page = page.clone();
            let toggle_key = key.clone();
            div()
                .id(SharedString::from(format!("book-detail-toc-toggle-{key}")))
                .w(px(32.0))
                .h(px(40.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(theme.text_muted)
                .child(if is_expanded { "⌄" } else { "›" })
                .on_click(move |_, _, cx| {
                    let _ = toggle_page.update(cx, |page, cx| page.toggle_toc_branch(toggle_key.clone(), cx));
                })
                .into_any_element()
        };
        rows.push(
            div()
                .id(row_id)
                .min_h(px(40.0))
                .pl(px(depth as f32 * 16.0))
                .flex()
                .items_center()
                .child(disclosure)
                .child(
                    div()
                        .id(SharedString::from(format!("book-detail-toc-link-{key}")))
                        .flex_1()
                        .min_w_0()
                        .py(px(8.0))
                        .cursor_pointer()
                        .text_size(gpui::rems(0.9))
                        .child(label)
                        .on_click(move |_, _, cx| open_ui.open_reader_at(open_hash.clone(), open_title.clone(), target.clone(), cx)),
                )
                .into_any_element(),
        );
        if is_expanded {
            rows.extend(toc_rows(&entry.children, depth + 1, &key, expanded, page, ui, content_hash, book_title, theme));
        }
    }
    rows
}

impl Render for BookDetailPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let back = self.on_back.clone();
        let cover = self.cover.clone();
        let Some(detail) = self.detail.clone() else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(16.0))
                .child(self.error.clone().unwrap_or_else(|| "Opening book details…".into()))
                .child(div().id("book-detail-back-loading").cursor_pointer().px(px(16.0)).py(px(10.0)).border_1().border_color(theme.border).child("Back").on_click(move |_, _, cx| back(cx)))
                .into_any_element();
        };
        let book = detail.book.clone();
        let current_interface_font = interface_font(cx);
        let description_width = description_column_width(window, current_interface_font.clone());
        if self.description_font.as_ref() != Some(&current_interface_font) {
            self.description = None;
            self.description_font = Some(current_interface_font.clone());
        }
        if !book.description.trim().is_empty() && self.description.is_none() {
            let provider = description_provider(book.description.trim());
            let foreground = u32::from(gpui::Rgba::from(theme.text));
            let background = u32::from(gpui::Rgba::from(theme.page_bg));
            let config = RendererInitialConfig {
                font_size: DESCRIPTION_FONT_SIZE,
                column_width: description_width as f64,
                max_column_count: Some(1),
                style_overrides: html::pipeline::ReaderStyleOverrides {
                    font_family: Some(current_interface_font.to_string()),
                    text_align: Some(html::pipeline::TextAlign::Justify),
                    line_height: Some(1.65),
                    foreground: Some(foreground),
                    background: Some(background),
                    ..Default::default()
                },
                paint_palette: html_view_core::ReaderPaintPalette { foreground: Some(foreground), background: Some(background) },
                ..Default::default()
            };
            self.description = Some(cx.new(|cx| HtmlView::from_provider(provider, vec!["description.html".to_owned()], 0, None, config, window, cx)));
        }
        let title = book.title.clone();
        let open_ui = self.ui.clone();
        let open_hash = book.content_hash.clone();
        let progress = book.progress.clamp(0.0, 100.0);
        let mut subjects = detail
            .subject_paths
            .iter()
            .map(|path| {
                let mut parts = path.rsplit(" / ");
                (path, parts.next().unwrap_or(path), parts.next())
            })
            .collect::<Vec<_>>();
        subjects.sort_by_key(|(_, leaf, _)| leaf.to_lowercase());
        let subject_labels = subjects
            .iter()
            .map(|(_, leaf, parent)| {
                let collisions = subjects.iter().filter(|(_, other, _)| other.eq_ignore_ascii_case(leaf)).count();
                if collisions > 1 { parent.map(|parent| format!("{leaf} · {parent}")).unwrap_or_else(|| (*leaf).to_owned()) } else { (*leaf).to_owned() }
            })
            .collect::<Vec<_>>();
        let toc_page = cx.entity().downgrade();
        let toc_open = self.toc_open;
        let toc_loading = self.toc_loading;
        let toc_error = self.toc_error.clone();
        let toc = if !toc_open {
            Vec::new()
        } else if toc_loading {
            vec![div().px(px(14.0)).py(px(12.0)).text_size(gpui::rems(0.85)).text_color(theme.text_muted).child("Loading contents…").into_any_element()]
        } else if let Some(error) = toc_error {
            vec![div().px(px(14.0)).py(px(12.0)).text_size(gpui::rems(0.85)).text_color(theme.text_muted).child(error).into_any_element()]
        } else if self.toc.is_empty() {
            vec![div().px(px(14.0)).py(px(12.0)).text_size(gpui::rems(0.85)).text_color(theme.text_muted).child("No table of contents available.").into_any_element()]
        } else {
            toc_rows(&self.toc, 0, "", &self.expanded_toc, &toc_page, &self.ui, &book.content_hash, &title, theme)
        };
        let back = self.on_back.clone();
        let description = self.description.clone();
        let toggle_toc = cx.entity().downgrade();
        let compact_layout = self.compact_layout;
        let layout_page = cx.entity().downgrade();
        let wide_layout_width = components::BOOK_COVER_WIDTH + DETAIL_HERO_GAP + description_width;

        let cover_column = div()
            .w(px(components::BOOK_COVER_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .id("book-detail-cover")
                    .cursor_pointer()
                    .child(match cover {
                        Some(cover) => components::book_cover(theme).child(components::book_cover_image(cover.image, cover.aspect_ratio, theme)),
                        None => components::missing_book_cover("Open book", theme),
                    })
                    .on_click(move |_, _, cx| open_ui.open_reader(open_hash.clone(), title.clone(), cx)),
            )
            .when(progress > 0.0, |column| {
                column.child(Progress::new("book-detail-progress").value(progress).color(theme.accent).with_size(Size::XSmall).w_full()).child(div().text_size(gpui::rems(0.75)).text_color(theme.text_muted).child(if progress >= 98.0 {
                    "Finished".to_owned()
                } else {
                    format!("{}%", progress.round() as i32)
                }))
            });
        let title_and_author = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(div().max_w(px(description_width)).text_size(gpui::rems(1.55)).line_height(gpui::relative(1.18)).font_weight(FontWeight::SEMIBOLD).child(book.title))
            .child(div().text_size(gpui::rems(0.9)).text_color(theme.text_muted).child(if book.author.is_empty() { "Unknown author".to_owned() } else { book.author }));
        let description_and_subjects = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .when_some(description, |copy, description| {
                let previous = description.clone();
                let next = description.clone();
                copy.child(
                    div().w_full().h(px(214.0)).flex().flex_col().gap(px(6.0)).child(div().flex_1().min_h_0().overflow_hidden().child(description)).child(
                        div()
                            .h(px(28.0))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .gap(px(6.0))
                            .child(div().id("book-description-previous").cursor_pointer().px(px(10.0)).flex().items_center().border_1().border_color(theme.border).rounded(px(components::RADIUS_SM)).child("‹").on_click(move |_, _, cx| {
                                let _ = previous.update(cx, |view, cx| view.previous_page(cx));
                            }))
                            .child(div().id("book-description-next").cursor_pointer().px(px(10.0)).flex().items_center().border_1().border_color(theme.border).rounded(px(components::RADIUS_SM)).child("›").on_click(move |_, _, cx| {
                                let _ = next.update(cx, |view, cx| view.next_page(cx));
                            })),
                    ),
                )
            })
            .when(!subject_labels.is_empty(), |copy| {
                copy.child(div().flex().flex_wrap().gap(px(8.0)).children(subject_labels.into_iter().map(|subject| div().px(px(10.0)).py(px(6.0)).border_1().border_color(theme.border).rounded(px(components::RADIUS_SM)).child(subject))))
            });
        let hero = if compact_layout {
            div().w_full().flex().flex_col().gap(px(18.0)).child(div().w_full().flex().items_start().gap(px(18.0)).child(cover_column).child(title_and_author.flex_1())).child(description_and_subjects)
        } else {
            div().w_full().flex().items_start().gap(px(DETAIL_HERO_GAP)).child(cover_column).child(div().flex_1().min_w_0().flex().flex_col().gap(px(12.0)).child(title_and_author).child(description_and_subjects))
        };

        components::home_page()
            .child(
                div()
                    .w_full()
                    .max_w(px(wide_layout_width))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap(px(24.0))
                    .on_prepaint(move |bounds, _, cx| {
                        let compact = f32::from(bounds.size.width) + 0.5 < wide_layout_width;
                        let _ = layout_page.update(cx, |page, cx| {
                            if page.compact_layout != compact {
                                page.compact_layout = compact;
                                cx.notify();
                            }
                        });
                    })
                    .child(div().id("book-detail-back").cursor_pointer().h(px(36.0)).flex().items_center().text_size(gpui::rems(0.85)).text_color(theme.text_muted).child("← Back").on_click(move |_, _, cx| back(cx)))
                    .child(hero)
                    .child(
                        div()
                            .border_1()
                            .border_color(theme.border)
                            .rounded(px(components::RADIUS_SM))
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("book-detail-toc-heading")
                                    .h(px(48.0))
                                    .px(px(14.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .cursor_pointer()
                                    .child(div().w(px(18.0)).text_color(theme.text_muted).child(if toc_open { "⌄" } else { "›" }))
                                    .child(div().text_size(gpui::rems(1.0)).font_weight(FontWeight::SEMIBOLD).child("Table of contents"))
                                    .on_click(move |_, _, cx| {
                                        let _ = toggle_toc.update(cx, |page, cx| page.toggle_toc(cx));
                                    }),
                            )
                            .when(toc_open, |panel| panel.child(div().border_t_1().border_color(theme.border).px(px(6.0)).py(px(4.0)).children(toc))),
                    )
                    .when(!detail.related.is_empty(), |page| {
                        page.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(12.0))
                                .child(div().text_size(gpui::rems(1.25)).font_weight(FontWeight::SEMIBOLD).child(if detail.authors.len() == 1 { format!("More by {}", detail.authors[0].name) } else { "More by these authors".to_owned() }))
                                .child(self.related.render_static("book-detail-related", window, cx)),
                        )
                    }),
            )
            .into_any_element()
    }
}
