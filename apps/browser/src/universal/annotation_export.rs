//! Shared, device-local export dialog for Settings and the library switcher.

use super::background_job::{self, BackgroundJobView};
use app::AppClient;
use book_model::BookFormat;
use gpui::prelude::*;
use gpui::{App, Context, Entity, SharedString, Subscription, Task, Window, div, px, rems};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::progress::Progress;
use gpui_component::{Disableable, WindowExt};
use library_backend::{BackendError, LibraryClient};
use library_model::{
    AnnotationExportBook, AnnotationExportConflict, AnnotationExportFormat, AnnotationExportOptions, AnnotationExportOutcome, AnnotationExportProgress, AnnotationExportReport, AutomaticAnnotationExportState,
    AutomaticAnnotationExportStatus, LibraryUpdate,
};
use std::{path::PathBuf, rc::Rc};
use sync_common::LibraryId;
use ui_components::{self as components, user_facing_error};
use uuid::Uuid;

pub(crate) fn open_annotation_export(backend: Rc<AppClient>, id: LibraryId, name: String, window: &mut Window, cx: &mut App) {
    background_job::open_job_dialog(650.0, move |window, cx| AnnotationExportDialog::new(backend, id, name, window, cx), window, cx);
}

struct AnnotationExportDialog {
    library: LibraryClient,
    name: String,
    folder: Entity<InputState>,
    books: Vec<AnnotationExportBook>,
    loading: bool,
    formats: [bool; 4],
    automatic: bool,
    active: Option<Uuid>,
    cancelling: bool,
    progress: Option<AnnotationExportProgress>,
    report: Option<AnnotationExportReport>,
    destination: Option<PathBuf>,
    error: Option<SharedString>,
    automatic_status: Option<AutomaticAnnotationExportStatus>,
    retrying: bool,
    retry: Option<Task<()>>,
    pending_options: Option<AnnotationExportOptions>,
    running_readers: Vec<AnnotationExportFormat>,
    checking_readers: bool,
    reader_check: Option<Task<()>>,
    _status: Task<()>,
    _input: Subscription,
    _load: Task<()>,
    export: Option<Task<()>>,
    poll: Option<Task<()>>,
}

impl BackgroundJobView for AnnotationExportDialog {
    type Progress = AnnotationExportProgress;
    fn library(&self) -> &LibraryClient {
        &self.library
    }
    fn active(&self) -> Option<Uuid> {
        self.active
    }
    fn cancelling(&self) -> bool {
        self.cancelling
    }
    fn receive_progress(&mut self, progress: Self::Progress) {
        self.progress = Some(progress);
    }
    fn confirm_job(&mut self, cx: &mut Context<Self>) -> bool {
        if self.report.is_some() {
            true
        } else {
            self.start_export(cx);
            false
        }
    }
    fn close_job(&mut self, cx: &mut Context<Self>) -> bool {
        self.request_close(cx)
    }
    async fn cancel(library: &LibraryClient, id: Uuid) -> Result<bool, BackendError> {
        library.cancel_annotation_export(id).await
    }
    async fn progress(library: &LibraryClient, id: Uuid) -> Result<Option<Self::Progress>, BackendError> {
        library.annotation_export_progress(id).await
    }
}

impl AnnotationExportDialog {
    fn new(backend: Rc<AppClient>, id: LibraryId, name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = backend.library(id);
        background_job::cancel_on_release(cx);
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder("Export folder"));
        let input = cx.subscribe_in(&folder, window, |view, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.start_export(cx);
            }
            cx.notify();
        });
        let source = library.clone();
        let load = cx.spawn(async move |view, cx| {
            let (books, folder, automatic) = futures_util::join!(source.annotation_export_snapshot(), backend.default_annotation_export_folder(), source.automatic_annotation_export());
            let _ = view.update_in(cx, |view, window, cx| {
                view.loading = false;
                match books {
                    Ok(books) => view.books = books,
                    Err(error) => view.error = Some(user_facing_error(&error, "Could not load annotations. Close the dialog and try again.").into()),
                }
                match folder {
                    Ok(Some(base)) => {
                        let destination = library_backend::annotation_export_destination(&PathBuf::from(base), &view.name);
                        view.folder.update(cx, |folder, cx| folder.set_value(destination.to_string_lossy().into_owned(), window, cx));
                    }
                    Ok(None) => {}
                    Err(error) => view.error = Some(user_facing_error(&error, "Could not read the default export folder. Choose an export folder.").into()),
                }
                match automatic {
                    Ok(status) => {
                        if let Some(options) = &status.options {
                            view.automatic = true;
                            view.formats = [AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Pdf, AnnotationExportFormat::Readest].map(|format| options.formats.contains(&format));
                            view.folder.update(cx, |folder, cx| folder.set_value(options.destination.to_string_lossy().into_owned(), window, cx));
                        }
                        view.automatic_status = Some(status);
                    }
                    Err(error) => view.error = Some(user_facing_error(&error, "Could not read automatic export settings.").into()),
                }
                cx.notify();
            });
        });
        let status_source = library.clone();
        let status = cx.spawn(async move |view, cx| {
            let Ok((updates, _)) = status_source.updates().await else { return };
            // Subscribe before the snapshot so changes during opening are seen.
            loop {
                if let Ok(status) = status_source.automatic_annotation_export().await {
                    if view
                        .update(cx, |view, cx| {
                            view.automatic_status = Some(status);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                loop {
                    let Ok(update) = updates.recv().await else { return };
                    if matches!(update, LibraryUpdate::AnnotationExportStatusChanged) {
                        break;
                    }
                }
                while updates.try_recv().is_ok() {}
            }
        });
        Self {
            library,
            name,
            folder,
            books: vec![],
            loading: true,
            formats: [true, false, false, false],
            automatic: false,
            active: None,
            cancelling: false,
            progress: None,
            report: None,
            destination: None,
            error: None,
            automatic_status: None,
            retrying: false,
            retry: None,
            pending_options: None,
            running_readers: vec![],
            checking_readers: false,
            reader_check: None,
            _status: status,
            _input: input,
            _load: load,
            export: None,
            poll: None,
        }
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
        if self.loading || self.active.is_some() || self.report.is_some() || self.pending_options.is_some() {
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
        let selection = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Choose annotation export folder".into()) });
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

    /// Keep the dialog until the worker acknowledges cancellation.
    fn request_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.active.is_some() {
            self.cancelling = true;
            cx.notify();
            false
        } else {
            self.back_to_options(cx);
            true
        }
    }

    fn start_export(&mut self, cx: &mut Context<Self>) {
        if self.pending_options.is_some() {
            self.check_readers(cx);
            return;
        }
        let Some(options) = self.options(cx) else { return };
        let needs_reader_check = options.install_readers && options.formats.iter().any(|format| matches!(format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) && self.format_count(*format) > 0);
        if needs_reader_check {
            self.pending_options = Some(options);
            self.check_readers(cx);
        } else {
            self.begin_export(options, cx);
        }
    }

    fn check_readers(&mut self, cx: &mut Context<Self>) {
        if self.checking_readers || self.active.is_some() {
            return;
        }
        let Some(options) = &self.pending_options else { return };
        let formats = options.formats.iter().copied().filter(|format| matches!(format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) && self.format_count(*format) > 0).collect();
        self.checking_readers = true;
        self.error = None;
        let library = self.library.clone();
        self.reader_check = Some(cx.spawn(async move |view, cx| {
            let result = library.annotation_export_running_readers(formats).await;
            let _ = view.update(cx, |view, cx| view.finish_reader_check(result, cx));
        }));
        cx.notify();
    }

    fn finish_reader_check(&mut self, result: Result<Vec<AnnotationExportFormat>, BackendError>, cx: &mut Context<Self>) {
        self.checking_readers = false;
        if self.pending_options.is_none() {
            return;
        }
        match result {
            Ok(running) => {
                self.running_readers = running;
                if self.running_readers.is_empty()
                    && let Some(options) = self.pending_options.take()
                {
                    self.begin_export(options, cx);
                }
            }
            Err(error) => self.error = Some(user_facing_error(&error, "Could not check reader apps. Try again.").into()),
        }
        cx.notify();
    }

    fn back_to_options(&mut self, cx: &mut Context<Self>) {
        self.reader_check = None;
        self.checking_readers = false;
        self.pending_options = None;
        self.running_readers.clear();
        self.error = None;
        cx.notify();
    }

    fn begin_export(&mut self, options: AnnotationExportOptions, cx: &mut Context<Self>) {
        let id = Uuid::new_v4();
        self.destination = Some(options.destination.clone());
        self.active = Some(id);
        self.cancelling = false;
        self.progress = None;
        self.error = None;
        let library = self.library.clone();
        let automatic = self.automatic;
        self.export = Some(cx.spawn(async move |view, cx| {
            let result = async {
                if !automatic {
                    library.set_automatic_annotation_export(None).await?;
                }
                let report = library.export_annotations(id, options.clone()).await?;
                if automatic && !report.cancelled {
                    library.set_automatic_annotation_export(Some(options)).await?;
                }
                Ok::<_, BackendError>(report)
            }
            .await;
            let _ = view.update(cx, |view, cx| {
                view.active = None;
                view.poll = None;
                match result {
                    Ok(report) => view.report = Some(report),
                    Err(error) => view.error = Some(user_facing_error(&error, "Could not export annotations. Check the destination and try again.").into()),
                }
                cx.notify();
            });
        }));
        let library = self.library.clone();
        self.poll = Some(background_job::poll_job(id, library, cx));
        cx.notify();
    }

    fn retry_automatic(&mut self, cx: &mut Context<Self>) {
        if self.retrying || self.active.is_some() {
            return;
        }
        self.retrying = true;
        let library = self.library.clone();
        self.retry = Some(cx.spawn(async move |view, cx| {
            let result = library.retry_automatic_annotation_export().await;
            let status = library.automatic_annotation_export().await.ok();
            let _ = view.update(cx, |view, cx| {
                view.retrying = false;
                if let Some(status) = status {
                    view.automatic_status = Some(status);
                }
                if let Err(error) = result {
                    view.error = Some(user_facing_error(&error, "Could not request another export attempt.").into());
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn render_automatic_status(&self, cx: &Context<Self>) -> Option<gpui::Div> {
        let status = self.automatic_status.as_ref()?;
        status.options.as_ref()?;
        let theme = components::browser_theme(cx);
        let mut body = div().flex().flex_col().gap(px(components::SPACE_SM)).child(components::section_title("Automatic export"));
        if let Some(error) = &status.error {
            body = body.child(components::error_text(theme).child(SharedString::from(error.clone())));
        }
        for output in &status.outputs {
            let format = match output.destination.format {
                AnnotationExportFormat::Markdown => "Markdown",
                AnnotationExportFormat::Pdf => "PDF",
                AnnotationExportFormat::Foliate => "Foliate",
                AnnotationExportFormat::Readest => "Readest",
            };
            let state = match output.state {
                AutomaticAnnotationExportState::Waiting => "Waiting",
                AutomaticAnnotationExportState::Retrying => "Retrying",
                AutomaticAnnotationExportState::NeedsAttention => "Needs attention",
            };
            let message = if output.failure.file_written { format!("File saved; retrying its tracking update. {}", output.message) } else { output.message.clone() };
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(components::SPACE_XS))
                    .child(components::form_hint(format!("{} · {format} · {state}", output.book_title), theme))
                    .child(components::form_hint(output.destination.path.to_string_lossy().into_owned(), theme))
                    .child(components::form_hint(message, theme)),
            );
        }
        if status.outputs.is_empty() && status.error.is_none() {
            body = body.child(components::form_hint(if status.last_success_at.is_some() { "Last export completed successfully." } else { "Automatic export is enabled." }, theme));
        } else {
            body = body.child(
                components::base_button("annotation-export-retry")
                    .label(if self.retrying { "Requesting retry…" } else { "Retry failed exports" })
                    .disabled(self.retrying || self.active.is_some())
                    .on_click(cx.listener(|view, _, _, cx| view.retry_automatic(cx))),
            );
        }
        Some(body)
    }

    fn render_options(&self, cx: &Context<Self>) -> gpui::Div {
        let theme = components::browser_theme(cx);
        let mut formats = div().debug_selector(|| "annotation-export-formats".into()).flex().flex_col().gap(px(components::SPACE_SM)).child(components::section_title("Export formats"));
        for (index, format, label, unit) in
            [(0, AnnotationExportFormat::Markdown, "Markdown", "books"), (1, AnnotationExportFormat::Foliate, "Foliate", "EPUBs"), (3, AnnotationExportFormat::Readest, "Readest", "EPUBs"), (2, AnnotationExportFormat::Pdf, "PDF", "PDFs")]
        {
            let count = self.format_count(format);
            formats = formats.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(components::SPACE_MD))
                    .child(Checkbox::new(SharedString::from(format!("annotation-export-format-{index}"))).label(label).checked(self.formats[index]).disabled(self.loading || (count == 0 && !self.automatic)).on_click(cx.listener(
                        move |view, selected, _, cx| {
                            view.formats[index] = *selected;
                            cx.notify();
                        },
                    )))
                    .child(components::form_hint(SharedString::from(format!("{count} {unit}")), theme)),
            );
        }
        let mut body = div().flex().flex_col().gap(px(components::SPACE_MD)).child(formats).child(
            Checkbox::new("annotation-export-automatic").label("Automatically export annotation changes").checked(self.automatic).disabled(self.loading).on_click(cx.listener(|view, selected, _, cx| {
                view.automatic = *selected;
                cx.notify();
            })),
        );
        if self.formats[0] {
            body = body.child(
                div().flex().flex_col().gap(px(components::SPACE_SM)).child(components::section_title("Export folder")).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(components::SPACE_SM))
                        .child(div().flex_1().min_w_0().child(Input::new(&self.folder).disabled(self.loading)))
                        .child(components::base_button("annotation-export-browse").label("Browse…").disabled(self.loading).on_click(cx.listener(|view, _, _, cx| view.choose_folder(cx)))),
                ),
            );
        }
        if !self.loading && self.books.is_empty() {
            body = body.child(components::form_hint("No annotations to export.", theme));
        }
        if self.loading {
            body = body.child(Progress::new("annotation-export-loading").loading(true).color(theme.accent));
        }
        body
    }

    fn render_progress(&self, cx: &Context<Self>) -> gpui::Div {
        let theme = components::browser_theme(cx);
        let progress = self.progress.as_ref();
        let total = progress.map_or(0, |progress| progress.total);
        let completed = progress.map_or(0, |progress| progress.completed);
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(components::SPACE_MD))
            .child(components::section_title(if self.cancelling { "Cancelling export…" } else { "Exporting annotations…" }))
            .child(Progress::new("annotation-export-progress").loading(total == 0).value(if total == 0 { 0.0 } else { completed as f32 / total as f32 * 100.0 }).color(theme.accent));
        if let Some(item) = progress.and_then(|progress| progress.current.as_ref()) {
            let title = self.books.iter().find(|book| book.book_id == item.book_id).map(|book| book.metadata.title.clone()).unwrap_or_else(|| item.path.to_string_lossy().into_owned());
            body = body.child(components::form_hint(title, theme));
        }
        if total > 0 {
            body = body.child(components::form_hint(SharedString::from(format!("{completed} of {total}")), theme));
        }
        body
    }

    fn render_reader_prompt(&self, cx: &Context<Self>) -> gpui::Div {
        let theme = components::browser_theme(cx);
        let title = if self.checking_readers {
            "Checking reader apps…"
        } else {
            match (self.running_readers.contains(&AnnotationExportFormat::Foliate), self.running_readers.contains(&AnnotationExportFormat::Readest)) {
                (true, true) => "Close Foliate and Readest",
                (true, false) => "Close Foliate",
                (false, true) => "Close Readest",
                _ => "Check reader apps",
            }
        };
        let mut body = div().debug_selector(|| "annotation-export-reader-prompt".into()).flex().flex_col().gap(px(components::SPACE_MD)).child(components::section_title(title));
        if self.checking_readers {
            body = body.child(Progress::new("annotation-export-reader-check").loading(true).color(theme.accent));
        } else {
            body = body.child(components::form_hint("Close the reader apps, then choose Continue to export annotations.", theme));
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
            .child(components::section_title(if report.cancelled {
                "Export cancelled"
            } else if report.results.iter().any(|result| matches!(result.outcome, AnnotationExportOutcome::Failed(_))) {
                "Export finished with errors"
            } else {
                "Annotations exported"
            }))
            .child(components::form_hint(SharedString::from(format!("{written} saved · {skipped} skipped · {failed} not saved")), theme));
        if report.results.iter().any(|result| matches!(result.destination.format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) && matches!(result.outcome, AnnotationExportOutcome::Written)) {
            body = body.child(components::form_hint("Reopen the book in Foliate or Readest to load the exported annotations.", theme));
        }
        for result in &report.results {
            if let AnnotationExportOutcome::Failed(error) = &result.outcome {
                let message = if result.failure.is_some_and(|failure| failure.file_written) {
                    format!("{}: File saved; finishing export failed: {error}", result.destination.path.display())
                } else {
                    format!("{}: {error}", result.destination.path.display())
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
    async fn reader_prompt_waits_for_closure_and_can_return_to_export_options(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = crate::navigation::LibrarySession::test_context(cx).await;
        assert!(library.backend().annotation_export_running_readers(vec![AnnotationExportFormat::Markdown]).await.unwrap().is_empty(), "unselected reader apps must not block an export");
        let output = tempfile::tempdir().unwrap();
        let backend = Rc::new(library.app().clone());
        let id = *library.backend().id();
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| AnnotationExportDialog::new(backend, id, "Library".into(), window, cx));
            gpui_component::Root::new(view, window, cx)
        });
        let view = window.read_with(cx, |root, _| root.view().clone().downcast::<AnnotationExportDialog>().ok().unwrap()).unwrap();
        cx.condition(&view, |view, _| !view.loading).await;
        view.update(cx, |view, cx| {
            view.formats = [true, true, false, true];
            view.pending_options = Some(AnnotationExportOptions {
                destination: output.path().join("Library"),
                formats: vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest],
                install_readers: true,
                conflict: AnnotationExportConflict::Replace,
                overrides: vec![],
            });
            for readers in [vec![AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest], vec![AnnotationExportFormat::Readest]] {
                view.checking_readers = true;
                view.finish_reader_check(Ok(readers.clone()), cx);
                assert_eq!(view.running_readers, readers);
                assert!(view.active.is_none());
                assert!(view.export.is_none());
                assert!(view.pending_options.is_some());
            }
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(gpui::size(px(360.0), px(420.0)));
        visual.run_until_parked();
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let prompt = visual.debug_bounds("annotation-export-reader-prompt").expect("the close-reader prompt must be visible");
        let footer = visual.debug_bounds("annotation-export-footer").unwrap();
        assert!(prompt.size.height > px(60.0));
        assert!(prompt.bottom() <= footer.top());
        assert!(footer.bottom() <= px(420.0));
        view.update(&mut visual, |view, cx| view.back_to_options(cx));
        view.read_with(&visual, |view, _| {
            assert!(view.pending_options.is_none());
            assert!(view.running_readers.is_empty());
            assert!(!view.checking_readers);
            assert_eq!(view.formats, [true, true, false, true], "returning must retain selected formats");
        });
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0, "the prompt must not start any export writes");
        assert!(library.backend().automatic_annotation_export().await.unwrap().options.is_none());
    }

    #[gpui::test]
    async fn dialog_keeps_export_options_visible_at_desktop_and_narrow_sizes(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = crate::navigation::LibrarySession::test_context(cx).await;
        let backend = Rc::new(library.app().clone());
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
        let (_directory, _session, library) = crate::navigation::LibrarySession::test_context(cx).await;
        let output = tempfile::tempdir().unwrap();
        library.app().set_library_asset_storage(*library.backend().id(), false).await.unwrap();
        library.app().set_default_annotation_export_folder(output.path().to_string_lossy().into_owned()).await.unwrap();
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
            content_hash: book,
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
        let backend = Rc::new(library.app().clone());
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
            view.automatic = true;
            view.start_export(cx);
        });
        cx.condition(&view, |view, _| view.active.is_none()).await;
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
        let automatic = library.backend().automatic_annotation_export().await.unwrap().options.unwrap();
        assert_eq!(automatic.destination, destination);
        assert_eq!(automatic.formats, vec![AnnotationExportFormat::Markdown]);
    }

    #[gpui::test]
    async fn dialog_selects_readest_without_requiring_an_export_folder(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = crate::navigation::LibrarySession::test_context(cx).await;
        let output = tempfile::tempdir().unwrap();
        library.app().set_library_asset_storage(*library.backend().id(), false).await.unwrap();
        library.app().set_default_annotation_export_folder(output.path().to_string_lossy().into_owned()).await.unwrap();
        let book = library.backend().import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), include_bytes!("../../../../client/library-backend/tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        library
            .backend()
            .upsert_annotation(Annotation {
                id: "readest-dialog-note".into(),
                content_hash: book,
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
        let backend = Rc::new(library.app().clone());
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
        let active = self.active.is_some();
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
                    .child(div().text_size(rems(1.5)).child("Export annotations"))
                    .child(components::form_hint(SharedString::from(if self.loading { self.name.clone() } else { format!("{} · {annotation_count} annotations in {} books", self.name, self.books.len()) }), theme)),
            )
            .child(components::outlined_icon_button("annotation-export-close", "Close", gpui_component::IconName::Close).disabled(self.cancelling && active).on_click(cx.listener(|view, _, window, cx| {
                if view.request_close(cx) {
                    window.close_dialog(cx);
                }
            })));
        let mut body = if active {
            self.render_progress(cx)
        } else if self.pending_options.is_some() {
            self.render_reader_prompt(cx)
        } else if let Some(report) = &self.report {
            self.render_report(report, cx)
        } else {
            self.render_options(cx)
        };
        if !active {
            if let Some(status) = self.render_automatic_status(cx) {
                body = div().flex().flex_col().gap(px(components::SPACE_MD)).child(body).child(status);
            }
        }
        let mut buttons = div().flex().justify_end().items_center().gap(px(components::SPACE_SM));
        if let Some(report) = &self.report {
            if report.results.iter().any(|result| result.destination.format == AnnotationExportFormat::Markdown && (matches!(result.outcome, AnnotationExportOutcome::Written) || result.failure.is_some_and(|failure| failure.file_written))) {
                let destination = self.destination.clone();
                buttons = buttons.child(components::base_button("annotation-export-open-folder").label("Open export folder").on_click(move |_, _, cx| {
                    if let Some(path) = &destination {
                        cx.open_with_system(path);
                    }
                }));
            }
            buttons = buttons.child(components::base_button("annotation-export-done").primary().label("Done").on_click(|_, window, cx| window.close_dialog(cx)));
        } else {
            if self.pending_options.is_some() && !self.checking_readers {
                buttons = buttons.child(components::base_button("annotation-export-back").label("Back").on_click(cx.listener(|view, _, _, cx| view.back_to_options(cx))));
            }
            buttons = buttons.child(components::dialog_cancel_button("annotation-export-cancel", if self.cancelling && active { "Cancelling…" } else { "Cancel" }).disabled(self.cancelling && active).on_click(cx.listener(
                |view, _, window, cx| {
                    if view.request_close(cx) {
                        window.close_dialog(cx);
                    }
                },
            )));
            if !active {
                buttons = buttons.child(
                    components::base_button("annotation-export-submit")
                        .primary()
                        .label(if self.checking_readers {
                            "Checking…"
                        } else if self.pending_options.is_some() {
                            "Continue"
                        } else if self.automatic {
                            "Save and export"
                        } else {
                            "Export"
                        })
                        .disabled(self.checking_readers || (self.pending_options.is_none() && self.options(cx).is_none()))
                        .on_click(cx.listener(|view, _, _, cx| view.start_export(cx))),
                );
            }
        }
        div()
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
