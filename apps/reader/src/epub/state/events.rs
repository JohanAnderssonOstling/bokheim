//! Projection of renderer events into EPUB reader state.

use crate::invalidation::notify;
use std::sync::Arc;

use gpui::{App, Context, Window};
use image::RgbaImage;
use skrift_column_paginator::{OpenedImage, RendererEvent};

use crate::epub::marks;
use crate::epub::{DocumentNavigation, LoadState, ReaderView};
use crate::invalidation::Component;
use crate::shell::persistence::StoredPosition;

impl ReaderView {
    pub(super) fn handle_renderer_event(&mut self, event: &RendererEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            RendererEvent::ExternalLink(href) => {
                if self.document_history.is_some() {
                    cx.emit(DocumentNavigation::Link(href.clone()));
                } else if crate::windows::native_windows_available() && crate::shell::dictionary::wikipedia::article_link(href).is_some() {
                    self.open_link_in_new_window(href.clone(), window, cx);
                } else if let Ok(url) = url::Url::parse(href) {
                    if matches!(url.scheme(), "https" | "http" | "mailto" | "tel") {
                        cx.open_url(url.as_str());
                    }
                }
            }
            RendererEvent::VisiblePageChanged => {
                if self.marks.recurring_annotations().next().is_some() {
                    self.refresh_recurring_annotations(false, cx);
                }
            }
            RendererEvent::NavigationJump { from } => {
                if self.document_history.is_some() {
                    cx.emit(DocumentNavigation::BeginJump(from.clone()));
                } else {
                    self.history.begin_jump(from.clone());
                }
            }
            RendererEvent::ReadingPositionChanged(position) => {
                if let Some(cfi) = self.book.cfi.source_position_cfi(&position.source) {
                    if self.document_history.is_some() {
                        cx.emit(DocumentNavigation::Settled(cfi));
                    } else if self.history.settle(&cfi) {
                        cx.notify();
                    }
                }
                if self.reading_position.as_ref() == Some(position) {
                    return;
                }
                let document_changed = self.reading_position.as_ref().is_none_or(|previous| previous.source.document != position.source.document);
                let toc_changed = self.toc.update_position(&position.source);
                self.toc.update_read_status(position.fraction >= 1.0);
                self.progress.update(position.fraction, position.document_fraction, position.location, position.total_locations, position.source.document, position.document_count);
                self.reading_position = Some(position.clone());
                if document_changed {
                    self.refresh_chapter_search(cx);
                }
                if toc_changed {
                    self.sync_toc_position(cx);
                }
                self.record_current_reading_position(cx);
                #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
                self.tts_navigation_changed(cx);
                notify(cx, Component::ReaderShell, "reading_position_changed");
            }
            RendererEvent::TitleChanged(title) => {
                let title = title.clone().unwrap_or_else(|| self.book_title.clone());
                if self.title != title {
                    self.title = title;
                    notify(cx, Component::ReaderShell, "title_changed");
                }
            }
            RendererEvent::SearchActiveChanged(active) => {
                if self.search.query.is_some() != *active {
                    if *active {
                        self.search.query = Some(self.search_input.read(cx).value().to_string());
                    } else {
                        self.search.clear();
                    }
                    notify(cx, Component::ReaderShell, "search_visibility_changed");
                }
            }
            // Controls persist requests before reflow. Delayed acknowledgements
            // must not revert them, and viewport-derived column widths belong
            // to each window rather than to the shared preferences.
            RendererEvent::FontSizeChanged(_) | RendererEvent::ColumnWidthChanged(_) | RendererEvent::ScaleChanged(_) => {}
            RendererEvent::SelectionFinished { cfi_range, .. } => {
                // A released handle can queue several ranges. Ignore snapshots
                // that were superseded or dismissed before they reach the shell.
                if let LoadState::Ready(renderer) = &self.load_state {
                    if renderer.read(cx).selection_cfi().as_deref() != Some(cfi_range.as_str()) {
                        return;
                    }
                }
                if marks::set_selection(&mut self.selection, cfi_range.clone()) {
                    self.annotation_popup_anchor = Some(window.mouse_position());
                    notify(cx, Component::ReaderShell, "selection_finished");
                }
            }
            RendererEvent::AnnotationActivated { id } => {
                self.annotation_popup_anchor = Some(window.mouse_position());
                let preferred = self.preferred_annotation_overlay(id, None, cx).unwrap_or_else(|| id.clone());
                self.select_annotation(preferred, window, cx);
            }
            RendererEvent::FootnoteOpened(preview) => {
                let prepared = match &self.load_state {
                    LoadState::Ready(renderer) => renderer.update(cx, |view, _| view.prepare_note()),
                    LoadState::Loading | LoadState::Error(_) => None,
                };
                self.footnote = prepared.map(|prepared| (preview.clone(), prepared));
                notify(cx, Component::ReaderShell, "footnote_opened");
            }
            RendererEvent::ImageOpened { uri, image } => {
                let save_image = std::mem::take(&mut self.save_opened_image);
                let image = match image {
                    OpenedImage::Raster { width, height, rgba } => {
                        let Some(buffer) = RgbaImage::from_raw(*width as u32, *height as u32, rgba.clone()) else { return };
                        let Ok(image) = gpui::Image::from_rgba(&buffer) else { return };
                        image
                    }
                    OpenedImage::Svg { bytes } => gpui::Image::from_bytes(gpui::ImageFormat::Svg, bytes.clone()),
                    OpenedImage::Encoded { bytes } => {
                        let extension = uri.rsplit('.').next().unwrap_or_default().split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
                        let format = match extension.as_str() {
                            "jpg" | "jpeg" => gpui::ImageFormat::Jpeg,
                            "webp" => gpui::ImageFormat::Webp,
                            "gif" => gpui::ImageFormat::Gif,
                            "svg" | "svgz" => gpui::ImageFormat::Svg,
                            "bmp" => gpui::ImageFormat::Bmp,
                            "tif" | "tiff" => gpui::ImageFormat::Tiff,
                            "ico" => gpui::ImageFormat::Ico,
                            "pnm" | "ppm" | "pgm" | "pbm" => gpui::ImageFormat::Pnm,
                            _ => gpui::ImageFormat::Png,
                        };
                        gpui::Image::from_bytes(format, bytes.clone())
                    }
                };
                if save_image {
                    ui_components::save_image(Arc::new(image), window, cx);
                    return;
                }
                self.image_backdrop = match &self.load_state {
                    LoadState::Ready(renderer) => renderer.update(cx, |view, _| view.page_snapshot()),
                    _ => None,
                };
                self.image_canvas.update(cx, |canvas, _| canvas.camera = Default::default());
                self.image_preview = Some((uri.clone(), Arc::new(image)));
                notify(cx, Component::ReaderShell, "image_preview_opened");
            }
            RendererEvent::MatchInfo { current, total } => {
                let next = Some((*current, *total));
                if self.search.match_position != next {
                    self.search.match_position = next;
                    notify(cx, Component::ReaderShell, "search_match_changed");
                }
            }
            RendererEvent::SearchResults(results) => {
                self.search.results = results.clone();
                self.search.rebuild_rows(&self.toc);
                notify(cx, Component::ReaderShell, "search_results_changed");
            }
            RendererEvent::SearchResultsAppended(results) => {
                self.search.results.append(results.clone());
                self.search.rebuild_rows(&self.toc);
                notify(cx, Component::ReaderShell, "search_results_changed");
            }
            RendererEvent::OperationFailed { operation, message } => {
                log::warn!("renderer failed to {operation}: {message}");
                if *operation == skrift_column_paginator::RendererOperation::UpdateDocument {
                    self.page_turn = None;
                    self.swipe_preview.clear();
                    cx.notify();
                }
                #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
                if *operation == skrift_column_paginator::RendererOperation::UpdateDocument {
                    self.tts_document_failed(&client_protocol::BackendError::operation(message.clone()), cx);
                }
            }
            _ => {}
        }
    }

    /// Save the source address, progress and resolved entry from one snapshot.
    pub(super) fn record_current_reading_position(&self, cx: &App) {
        if let Some(position) = &self.reading_position
            && let LoadState::Ready(view) = &self.load_state
            && let Some(cfi) = self.book.cfi.source_position_cfi(&position.source)
        {
            self.reading_positions.record(StoredPosition::epub(self.book.store_cfi(cfi), position.fraction).within(self.toc.active_link()), view.read(cx).navigation_revision());
        }
    }
}
