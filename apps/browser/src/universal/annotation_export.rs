//! Shared, device-local export dialog for Settings and the library switcher.

use ui_text::text;

use crate::navigation::{Libraries, LibraryOperationDialog};
use book_model::BookFormat;
use futures_util::FutureExt;
use gpui::prelude::*;
use gpui::{App, Context, DismissEvent, Entity, FocusHandle, Focusable, KeyDownEvent, SharedString, Subscription, Task, Window, div, px, rems};
use gpui_component::Disableable;
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{InputEvent, InputState};
use library_backend::{BackendError, LibraryClient};
use library_management::management::ManagementClient;
use library_model::{AnnotationExportBook, AnnotationExportConflict, AnnotationExportFailureKind, AnnotationExportFormat, AnnotationExportOptions, AnnotationExportOutcome, AnnotationExportReport};
use std::path::PathBuf;
use sync_common::LibraryId;
use ui_components::{self as components, user_facing_error};

fn export_failure_message(kind: AnnotationExportFailureKind) -> &'static str {
    match kind {
        AnnotationExportFailureKind::Busy => text!(browse::export_destination_busy),
        AnnotationExportFailureKind::Unavailable => text!(browse::export_destination_unavailable),
        AnnotationExportFailureKind::Changed => text!(browse::export_changed),
        AnnotationExportFailureKind::InvalidData => text!(browse::export_book_unreadable),
        AnnotationExportFailureKind::Identity => text!(browse::export_book_mismatch),
        AnnotationExportFailureKind::Unsupported => text!(browse::export_format_unsupported),
        AnnotationExportFailureKind::LimitExceeded => text!(browse::export_limit_reached),
        AnnotationExportFailureKind::Io => text!(browse::export_save_failed),
        AnnotationExportFailureKind::Tracking => text!(browse::export_finish_failed),
    }
}

pub(crate) fn open_annotation_export(libraries: Entity<Libraries>, management: ManagementClient, id: LibraryId, name: String, window: &mut Window, cx: &mut App) {
    let selection_epoch = libraries.read(cx).selection_epoch();
    window.defer(cx, move |window, cx| {
        window.defer(cx, move |window, cx| {
            let library = match management.library(id) {
                Ok(library) => library,
                Err(error) => {
                    crate::services::notify_error("annotation-export", user_facing_error(&error, text!(browse::could_not_load_annotations_close_the_dialog_and_try_again)), window, cx);
                    return;
                }
            };
            let view = cx.new(|cx| AnnotationExportDialog::new(library, name, window, cx));
            libraries.update(cx, |libraries, cx| libraries.show_operation_dialog(id, view, 650.0, selection_epoch, window, cx));
        });
    });
}

struct AnnotationExportDialog {
    focus: FocusHandle,
    library: LibraryClient,
    name: String,
    folder: Entity<InputState>,
    books: Vec<AnnotationExportBook>,
    loading: bool,
    formats: [bool; 4],
    exporting: bool,
    report: Option<AnnotationExportReport>,
    destination: Option<PathBuf>,
    error: Option<SharedString>,
    _input: Subscription,
    _load: Task<()>,
}

impl gpui::EventEmitter<DismissEvent> for AnnotationExportDialog {}
impl LibraryOperationDialog for AnnotationExportDialog {}
impl Focusable for AnnotationExportDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AnnotationExportDialog {
    fn new(library: LibraryClient, name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder(text!(browse::export_folder)));
        let input = cx.subscribe_in(&folder, window, |view, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.start_export(cx);
            }
            cx.notify();
        });
        let preferences = crate::services::preference_client(cx);
        let source = library.clone();
        let load = cx.spawn(async move |view, cx| {
            let (books, folder) = futures_util::join!(source.annotation_export_snapshot(), preferences.default_annotation_export_folder().map(|result| result.map_err(BackendError::operation)));
            let _ = view.update_in(cx, |view, window, cx| {
                view.loading = false;
                match books {
                    Ok(books) => view.books = books,
                    Err(error) => view.error = Some(user_facing_error(&error, text!(browse::could_not_load_annotations_close_the_dialog_and_try_again)).into()),
                }
                match folder {
                    Ok(Some(base)) => {
                        let destination = library_backend::annotation_export_destination(&PathBuf::from(base), &view.name);
                        view.folder.update(cx, |folder, cx| folder.set_value(destination.to_string_lossy().into_owned(), window, cx));
                    }
                    Ok(None) => {}
                    Err(error) => view.error = Some(user_facing_error(&error, text!(browse::could_not_read_the_default_export_folder_choose_an_export_folder)).into()),
                }
                cx.notify();
            });
        });
        Self { focus: cx.focus_handle(), library, name, folder, books: vec![], loading: true, formats: [true, false, false, false], exporting: false, report: None, destination: None, error: None, _input: input, _load: load }
    }

    fn format_count(&self, format: AnnotationExportFormat) -> usize {
        self.books
            .iter()
            .filter(|book| match format {
                AnnotationExportFormat::Markdown => true,
                AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest => book.format == BookFormat::Epub,
                AnnotationExportFormat::Pdf => book.format == BookFormat::Pdf,
            })
            .count()
    }

    fn options(&self, cx: &App) -> Option<AnnotationExportOptions> {
        if self.loading || self.exporting || self.report.is_some() {
            return None;
        }
        let formats = [AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Pdf, AnnotationExportFormat::Readest]
            .into_iter()
            .zip(self.formats)
            .filter_map(|(format, selected)| selected.then_some(format))
            .collect::<Vec<_>>();
        let destination = PathBuf::from(self.folder.read(cx).value().trim());
        if formats.is_empty() || (formats.contains(&AnnotationExportFormat::Markdown) && !destination.is_absolute()) {
            return None;
        }
        Some(AnnotationExportOptions { destination, formats, install_readers: true, conflict: AnnotationExportConflict::Replace, overrides: vec![] })
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(text!(browse::choose_annotation_export_folder).into()) });
        cx.spawn(async move |view, cx| {
            let Ok(Ok(Some(paths))) = selection.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = view.update_in(cx, |view, window, cx| {
                view.folder.update(cx, |folder, cx| folder.set_value(path.to_string_lossy().into_owned(), window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    fn start_export(&mut self, cx: &mut Context<Self>) {
        let Some(options) = self.options(cx) else { return };
        self.destination = Some(options.destination.clone());
        self.exporting = true;
        self.error = None;
        let library = self.library.clone();
        // An accepted export finishes even when the dialog is closed.
        cx.spawn(async move |view, cx| {
            let result = library.export_annotations(options).await;
            if let Ok(report) = &result {
                for result in &report.results {
                    if let AnnotationExportOutcome::Failed(error) = &result.outcome {
                        log::warn!("Annotation export failed for {}: {error}", result.destination.path.display());
                    }
                }
            }
            let _ = view.update(cx, |view, cx| {
                view.exporting = false;
                match result {
                    Ok(report) => view.report = Some(report),
                    Err(error) => view.error = Some(user_facing_error(&error, text!(browse::could_not_export_annotations_check_the_destination_and_try_again)).into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_options(&self, window: &Window, cx: &Context<Self>) -> gpui::Div {
        let theme = components::browser_theme(cx);
        let mut formats = div().debug_selector(|| "annotation-export-formats".into()).flex().flex_col().gap(px(components::SPACE_SM)).child(components::section_title(text!(browse::export_formats)));
        for (index, format, label, unit) in [
            (0, AnnotationExportFormat::Markdown, text!(browse::markdown), "books"),
            (1, AnnotationExportFormat::Foliate, text!(browse::foliate), text!(browse::epubs)),
            (3, AnnotationExportFormat::Readest, text!(browse::readest), text!(browse::epubs)),
            (2, AnnotationExportFormat::Pdf, text!(browse::pdf), text!(browse::pdfs)),
        ] {
            let count = self.format_count(format);
            formats = formats.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(components::SPACE_MD))
                    .child(Checkbox::new(SharedString::from(format!("annotation-export-format-{index}"))).label(label).checked(self.formats[index]).disabled(self.loading || count == 0).on_click(cx.listener(move |view, selected, _, cx| {
                        view.formats[index] = *selected;
                        cx.notify();
                    })))
                    .child(components::form_hint(SharedString::from(format!(text!(browse::count_with_unit), count = count, unit = unit)), theme)),
            );
        }
        let mut body = div().flex().flex_col().gap(px(components::SPACE_MD)).child(formats);
        if self.formats[0] {
            body = body.child(
                div().flex().flex_col().gap(px(components::SPACE_SM)).child(components::section_title(text!(browse::export_folder))).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(components::SPACE_SM))
                        .child(div().flex_1().min_w_0().child(components::browser_path_input("annotation-export-folder-label", &self.folder, self.loading, theme, window, cx)))
                        .child(components::base_button("annotation-export-browse").label(text!(browse::browse)).disabled(self.loading).on_click(cx.listener(|view, _, _, cx| view.choose_folder(cx)))),
                ),
            );
        }
        if !self.loading && self.books.is_empty() {
            body = body.child(components::form_hint(text!(browse::no_annotations_to_export), theme));
        }
        if self.loading {
            body = body.child(components::form_hint(text!(common::loading), theme));
        }
        body
    }

    fn render_report(&self, report: &AnnotationExportReport, cx: &Context<Self>) -> gpui::Div {
        let theme = components::browser_theme(cx);
        let written = report.results.iter().filter(|result| matches!(result.outcome, AnnotationExportOutcome::Written) || result.failure.is_some_and(|failure| failure.file_written)).count();
        let skipped = report.results.iter().filter(|result| matches!(result.outcome, AnnotationExportOutcome::Skipped)).count();
        let failed = report.results.len() - written - skipped;
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(components::SPACE_SM))
            .child(components::section_title(if report.results.iter().any(|result| matches!(result.outcome, AnnotationExportOutcome::Failed(_))) { text!(browse::export_finished_with_errors) } else { text!(browse::annotations_exported) }))
            .child(components::form_hint(SharedString::from(format!(text!(browse::saved_skipped_not_saved), failed = failed, skipped = skipped, written = written)), theme));
        if report.results.iter().any(|result| matches!(result.destination.format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) && matches!(result.outcome, AnnotationExportOutcome::Written)) {
            body = body.child(components::form_hint(text!(browse::reopen_the_book_in_foliate_or_readest_to_load_the_exported_annotations), theme));
        }
        for result in &report.results {
            if let AnnotationExportOutcome::Failed(_) = &result.outcome {
                let message = if result.failure.is_some_and(|failure| failure.file_written) {
                    format!(text!(browse::file_saved_finishing_export_failed), components::path_label(&result.destination.path))
                } else {
                    format!("{}: {}", components::path_label(&result.destination.path), export_failure_message(result.failure.map_or(AnnotationExportFailureKind::Io, |failure| failure.kind)))
                };
                body = body.child(components::error_text(theme).child(SharedString::from(message)));
            }
        }
        body
    }
}

#[cfg(all(test, not(feature = "kobo")))]
mod tests {
    use super::*;
    use book_model::{Annotation, AnnotationAnchor, AnnotationStyle};
    use gpui::TestAppContext;
    use std::fs;
    use sync_common::ROOT_DIR_ID;

    #[gpui::test]
    async fn dialog_keeps_export_options_visible_at_desktop_and_narrow_sizes(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library, _app_client) = crate::navigation::LibrarySession::test_context(cx).await;
        let backend = library.management().clone();
        let id = *library.backend().id();
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| AnnotationExportDialog::new(backend, id, "Library".into(), window, cx));
            gpui_component::Root::new(view, window, cx)
        });
        let view = window.read_with(cx, |root, _| root.view().clone().downcast::<AnnotationExportDialog>().ok().unwrap()).unwrap();
        cx.condition(&view, |view, _| !view.loading).await;
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        for (width, height) in [(800.0, 700.0), (360.0, 420.0)] {
            visual.simulate_resize(gpui::size(px(width), px(height)));
            visual.run_until_parked();
            visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
            let body = visual.debug_bounds("annotation-export-body").expect("export body must render");
            let formats = visual.debug_bounds("annotation-export-formats").expect("format controls must render");
            let footer = visual.debug_bounds("annotation-export-footer").expect("export actions must render");
            assert!(body.size.height > px(100.0), "export body collapsed at {width}×{height}: {body:?}");
            assert!(formats.size.height > px(80.0), "format controls collapsed: {formats:?}");
            assert!(body.bottom() <= footer.top(), "export options must not overlap the footer");
            assert!(footer.bottom() <= px(height), "export actions must fit the window");
        }
    }

    #[gpui::test]
    async fn dialog_loads_the_library_destination_and_overwrites_markdown(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library, _app_client) = crate::navigation::LibrarySession::test_context(cx).await;
        let output = tempfile::tempdir().unwrap();
        library.management().set_library_asset_storage(*library.backend().id(), false).await.unwrap();
        cx.read(crate::services::preference_client).set_default_annotation_export_folder(output.path().to_string_lossy().into_owned()).await.unwrap();
        let book = library
            .backend()
            .import_downloaded_book(
                ROOT_DIR_ID,
                "Notes.fb2".into(),
                br#"<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0"><description><title-info><book-title>Export fixture</book-title><lang>en</lang></title-info></description><body><section><p>Highlighted passage</p></section></body></FictionBook>"#.to_vec(),
            )
            .await
            .unwrap();
        let annotation = Annotation {
            id: "export-dialog-note".into(),
            book_id: book,
            anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:1)"),
            exact_text: "Highlighted passage".into(),
            style: AnnotationStyle::Highlight,
            color: 0xffcc00,
            note: "User note".into(),
            created_at: 1,
            modified_at: 2,
            deleted: false,
            toc_ordinal: None,
            progress: None,
        };
        library.backend().upsert_annotation(annotation).await.unwrap();
        let backend = library.management().clone();
        let id = *library.backend().id();
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| AnnotationExportDialog::new(backend, id, "My library".into(), window, cx));
            gpui_component::Root::new(view, window, cx)
        });
        let view = window.read_with(cx, |root, _| root.view().clone().downcast::<AnnotationExportDialog>().ok().unwrap()).unwrap();
        cx.condition(&view, |view, _| !view.loading).await;
        let destination = output.path().join("My library");
        view.read_with(cx, |view, cx| {
            assert!(view.error.is_none(), "{:?}", view.error);
            let options = view.options(cx).unwrap();
            assert_eq!(options.destination, destination);
            assert_eq!(options.conflict, AnnotationExportConflict::Replace);
        });
        fs::create_dir_all(&destination).unwrap();
        let target = destination.join("Export fixture.md");
        fs::write(&target, "An older export").unwrap();
        view.update(cx, |view, cx| {
            view.start_export(cx);
        });
        cx.condition(&view, |view, _| !view.exporting).await;
        view.read_with(cx, |view, _| {
            assert!(view.error.is_none(), "{:?}", view.error);
            let report = view.report.as_ref().unwrap();
            assert_eq!(report.results.len(), 1);
            assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Written));
            assert_eq!(report.results[0].destination.path, target);
        });
        let markdown = fs::read_to_string(&target).unwrap();
        assert!(markdown.contains("> *Highlighted passage*"));
        assert!(markdown.contains("User note"));
        assert!(!markdown.contains("An older export"));
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1, "overwrite must not create numbered copies");
    }

    #[gpui::test]
    async fn dialog_selects_readest_without_requiring_an_export_folder(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library, _app_client) = crate::navigation::LibrarySession::test_context(cx).await;
        let output = tempfile::tempdir().unwrap();
        library.management().set_library_asset_storage(*library.backend().id(), false).await.unwrap();
        cx.read(crate::services::preference_client).set_default_annotation_export_folder(output.path().to_string_lossy().into_owned()).await.unwrap();
        let book = library.backend().import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), include_bytes!("../../../../client/library-backend/tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        library
            .backend()
            .upsert_annotation(Annotation {
                id: "readest-dialog-note".into(),
                book_id: book,
                anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:1)"),
                exact_text: "Highlighted passage".into(),
                style: AnnotationStyle::Underline,
                color: 0xffcc00,
                note: "User note".into(),
                created_at: 1,
                modified_at: 2,
                deleted: false,
                toc_ordinal: None,
                progress: None,
            })
            .await
            .unwrap();
        let backend = library.management().clone();
        let id = *library.backend().id();
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| {
                let mut view = AnnotationExportDialog::new(backend, id, "My library".into(), window, cx);
                view.formats = [false, false, false, true];
                view
            });
            gpui_component::Root::new(view, window, cx)
        });
        let view = window.read_with(cx, |root, _| root.view().clone().downcast::<AnnotationExportDialog>().ok().unwrap()).unwrap();
        cx.condition(&view, |view, _| !view.loading).await;
        view.read_with(cx, |view, cx| {
            assert_eq!(view.format_count(AnnotationExportFormat::Readest), 1);
            assert_eq!(view.options(cx).unwrap().formats, vec![AnnotationExportFormat::Readest]);
        });
        window.update(cx, |_, window, cx| view.update(cx, |view, cx| view.folder.update(cx, |folder, cx| folder.set_value("", window, cx)))).unwrap();
        view.read_with(cx, |view, cx| {
            let options = view.options(cx).unwrap();
            assert!(options.install_readers);
            assert_eq!(options.formats, vec![AnnotationExportFormat::Readest]);
        });
    }
}

impl Render for AnnotationExportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let annotation_count = self.books.iter().flat_map(|book| &book.annotations).filter(|annotation| !annotation.deleted).count();
        let heading = div()
            .flex()
            .justify_between()
            .items_start()
            .gap(px(components::SPACE_MD))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(components::SPACE_XS))
                    .child(div().text_size(rems(1.5)).child(text!(browse::export_annotations)))
                    .child(components::form_hint(SharedString::from(if self.loading { self.name.clone() } else { format!(text!(browse::annotations_in_books), self.name, self.books.len(), annotation_count = annotation_count) }), theme)),
            )
            .child(components::outlined_icon_button("annotation-export-close", text!(common::close), gpui_component::IconName::Close).on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))));
        let body = if self.exporting {
            div().child(components::form_hint(text!(browse::exporting_annotations), theme))
        } else if let Some(report) = &self.report {
            self.render_report(report, cx)
        } else {
            self.render_options(window, cx)
        };
        let mut buttons = div().flex().justify_end().items_center().gap(px(components::SPACE_SM));
        if let Some(report) = &self.report {
            if report.results.iter().any(|result| result.destination.format == AnnotationExportFormat::Markdown && (matches!(result.outcome, AnnotationExportOutcome::Written) || result.failure.is_some_and(|failure| failure.file_written))) {
                let destination = self.destination.clone();
                buttons = buttons.child(components::base_button("annotation-export-open-folder").label(text!(browse::open_export_folder)).on_click(move |_, _, cx| {
                    if let Some(path) = &destination {
                        cx.open_with_system(path);
                    }
                }));
            }
            buttons = buttons.child(components::base_button("annotation-export-done").primary().label(text!(browse::done)).on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))));
        } else {
            buttons = buttons.child(components::dialog_cancel_button("annotation-export-close-dialog", text!(common::close)).on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))));
            buttons = buttons.child(
                components::base_button("annotation-export-submit")
                    .primary()
                    .label(if self.exporting { text!(browse::exporting_annotations) } else { text!(browse::export) })
                    .disabled(self.options(cx).is_none())
                    .on_click(cx.listener(|view, _, _, cx| view.start_export(cx))),
            );
        }
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| match event.keystroke.key.as_str() {
                "enter" if view.report.is_some() => {
                    cx.emit(DismissEvent);
                    cx.stop_propagation();
                }
                "enter" => {
                    view.start_export(cx);
                    cx.stop_propagation();
                }
                "escape" => {
                    cx.emit(DismissEvent);
                    cx.stop_propagation();
                }
                _ => {}
            }))
            .w_full()
            .min_w_0()
            .max_h(window.viewport_size().height - px(2.0 * components::SPACE_MD))
            .flex()
            .flex_col()
            .text_color(theme.text)
            .bg(theme.page_bg)
            .child(div().flex_none().p(px(components::SPACE_MD)).border_b_1().border_color(theme.rule).child(heading))
            .child(
                div()
                    .id("annotation-export-body")
                    .debug_selector(|| "annotation-export-body".into())
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(components::SPACE_MD))
                    .child(body)
                    .children(self.error.clone().map(|error| components::error_text(theme).mt(px(components::SPACE_SM)).child(error))),
            )
            .child(div().debug_selector(|| "annotation-export-footer".into()).flex_none().p(px(components::SPACE_MD)).border_t_1().border_color(theme.rule).child(buttons))
    }
}
