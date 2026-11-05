//! A reader import dialog owned by, and rendered inside, its destination library.

use library_management::management::ManagementClient;
use ui_text::text;

use super::LibrarySession;
use crate::services::{import_failure_message, notify_error, notify_info, notify_warning};
use gpui::prelude::*;
use gpui::{App, Context, DismissEvent, FocusHandle, Focusable, KeyDownEvent, Role, SharedString, Task, Toggled, Window, div, px, rems};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{Disableable, IconName};
use library_backend::{BackendError, DirId, LibraryClient, ReaderBookCounts, ReaderImportMode, ReaderImportOptions, ReaderImportSource, ReaderLibrarySummary};
use ui_components::{self as components, ScrollableElement, user_facing_error};

impl LibrarySession {
    pub(crate) fn open_reader_import(&mut self, source: ReaderImportSource, parent: DirId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((dialog, _)) = &self.reader_import {
            dialog.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        if self.library.files_busy(cx) || self.library.importing(cx) {
            return;
        }
        let management = self.library.management().clone();
        let library = self.library.backend().clone();
        let dialog = cx.new(|cx| ReaderImportDialog::new(management, library, source, parent, cx));
        let closed = cx.subscribe_in(&dialog, window, |session, dialog, _: &DismissEvent, window, cx| {
            let request = dialog.update(cx, |dialog, _| dialog.pending_import.take().map(|options| (dialog.library.clone(), dialog.source, options)));
            if let Some((library, source, options)) = request {
                let updates = session.library.updates();
                let completion = updates.downgrade();
                let task = cx.spawn(async move |_, cx| {
                    let result = library.import_reader(source, options).await;
                    let _ = completion.update_in(cx, |updates, window, cx| {
                        updates.set_import_task(None, cx);
                        match result {
                            Ok(report) => {
                                let summary = format!(text!(browse::imported_already_present_failed), report.imported, report.already_present, report.failures.len());
                                let message =
                                    std::iter::once(format!("{}: {summary}", source.name())).chain(report.failures.iter().take(3).map(import_failure_message)).chain(report.warnings.iter().take(3).cloned()).collect::<Vec<_>>().join("\n");
                                if report.failures.is_empty() && report.warnings.is_empty() {
                                    notify_info("reader-import-complete", message, window, cx);
                                } else {
                                    notify_warning("reader-import-warning", message, window, cx);
                                }
                            }
                            Err(error) => notify_error("reader-import-error", format!("{}: {}", source.name(), user_facing_error(&error, text!(browse::could_not_import_the_selected_books_please_try_again))), window, cx),
                        }
                    });
                });
                updates.update(cx, |updates, cx| updates.set_import_task(Some(task), cx));
            }
            session.reader_import = None;
            session.focus_page(window, cx);
            cx.notify();
        });
        dialog.read(cx).focus_handle(cx).focus(window, cx);
        self.reader_import = Some((dialog, closed));
        cx.notify();
    }

    pub(super) fn dismiss_reader_import(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((dialog, _)) = &self.reader_import else { return false };
        dialog.update(cx, |dialog, cx| dialog.dismiss(cx));
        true
    }
}

pub(super) struct ReaderImportDialog {
    focus: FocusHandle,
    source: ReaderImportSource,
    library: LibraryClient,
    parent: DirId,
    summary: Option<ReaderLibrarySummary>,
    loading: bool,
    group: Option<String>,
    mode: ReaderImportMode,
    mode_focus: [FocusHandle; 2],
    pub(super) importing: bool,
    pending_import: Option<ReaderImportOptions>,
    error: Option<SharedString>,
    _load: Task<()>,
}

impl gpui::EventEmitter<DismissEvent> for ReaderImportDialog {}

impl Focusable for ReaderImportDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ReaderImportDialog {
    fn new(management: ManagementClient, library: LibraryClient, source: ReaderImportSource, parent: DirId, cx: &mut Context<Self>) -> Self {
        Self::with_source(library, source, parent, async move { management.reader_import_summary(source).await }, cx)
    }

    fn with_source(library: LibraryClient, source: ReaderImportSource, parent: DirId, summary: impl std::future::Future<Output = Result<ReaderLibrarySummary, BackendError>> + 'static, cx: &mut Context<Self>) -> Self {
        let load = cx.spawn(async move |view, cx| {
            let result = summary.await;
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(summary) => view.summary = Some(summary),
                    Err(error) => view.error = Some(format!("{}: {}", source.name(), user_facing_error(&error, text!(browse::could_not_read_the_library_in_its_default_location))).into()),
                }
                cx.notify();
            });
        });
        Self {
            focus: cx.focus_handle(),
            source,
            library,
            parent,
            summary: None,
            loading: true,
            group: None,
            mode: ReaderImportMode::Copy,
            mode_focus: [cx.focus_handle(), cx.focus_handle()],
            importing: false,
            pending_import: None,
            error: None,
            _load: load,
        }
    }

    fn mode_choice(&self, mode: ReaderImportMode, label: &'static str, disabled: bool, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let index = usize::from(mode == ReaderImportMode::Move);
        let focus = self.mode_focus[index].clone();
        let selected = self.mode == mode;
        div()
            .id(("reader-mode", index))
            .role(Role::RadioButton)
            .aria_label(label)
            .aria_toggled(if selected { Toggled::True } else { Toggled::False })
            .flex()
            .items_center()
            .gap(px(components::SPACE_SM))
            .p(px(2.0))
            .rounded(px(3.0))
            .border_1()
            .border_color(if focus.is_focused(window) { theme.accent } else { theme.page_bg })
            .when(disabled, |row| row.opacity(0.5))
            .when(!disabled, |row| {
                row.cursor_pointer()
                    .track_focus(&focus.tab_stop(selected))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.mode = mode;
                        view.mode_focus[index].focus(window, cx);
                        cx.notify();
                    }))
                    .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                        let next = match event.keystroke.key.as_str() {
                            "space" | "enter" => mode,
                            "left" | "up" | "right" | "down" if view.summary.as_ref().is_some_and(|summary| summary.can_move) => {
                                if view.mode == ReaderImportMode::Copy {
                                    ReaderImportMode::Move
                                } else {
                                    ReaderImportMode::Copy
                                }
                            }
                            _ => return,
                        };
                        view.mode = next;
                        view.mode_focus[usize::from(next == ReaderImportMode::Move)].focus(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }))
            })
            .child(
                div()
                    .size(px(16.0))
                    .rounded_full()
                    .border_1()
                    .border_color(if selected { theme.accent } else { theme.rule })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |circle| circle.child(div().size(px(8.0)).rounded_full().bg(theme.accent))),
            )
            .child(label)
    }

    fn counts(&self) -> ReaderBookCounts {
        self.summary
            .as_ref()
            .map(|summary| match &self.group {
                None => summary.counts,
                Some(group) => summary.groups.iter().find(|entry| &entry.name == group).map(|group| group.counts).unwrap_or_default(),
            })
            .unwrap_or_default()
    }

    fn options(&self) -> Option<ReaderImportOptions> {
        if self.loading || self.importing || self.pending_import.is_some() || self.counts().total() == 0 {
            return None;
        }
        Some(ReaderImportOptions { parent_id: self.parent, group: self.group.clone(), mode: self.mode })
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if !self.importing {
            cx.emit(DismissEvent);
        }
    }

    fn start_import(&mut self, cx: &mut Context<Self>) {
        let Some(options) = self.options() else { return };
        self.pending_import = Some(options);
        self.importing = true;
        // The session retains the task after closing this configuration dialog.
        cx.emit(DismissEvent);
    }
}

impl Render for ReaderImportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let locked = self.loading || self.summary.is_none() || self.importing || self.pending_import.is_some();
        let heading = div().flex().items_center().justify_between().gap(px(components::SPACE_MD)).child(div().text_size(rems(1.5)).child(format!(text!(browse::import_from), self.source.name()))).child(
            components::outlined_icon_button("reader-close", text!(common::close), IconName::Close).disabled(self.importing).on_click(cx.listener(|view, _, _, cx| {
                if !view.importing {
                    view.dismiss(cx);
                }
            })),
        );
        let label = self.group.as_deref().map(|group| if group.is_empty() { text!(browse::ungrouped).to_owned() } else { group.replace('/', " / ") }).unwrap_or_else(|| text!(browse::all_groups).into());
        let summary = self.summary.clone();
        let selected = self.group.clone();
        let view = cx.entity();
        let selector = components::base_button("reader-group").label(label).icon(IconName::ChevronDown).disabled(locked).dropdown_menu(move |mut menu, _, _| {
            let all = view.clone();
            menu = menu.item(
                components::menu_item(text!(browse::all_groups), move |_, _, cx| {
                    all.update(cx, |view, cx| {
                        if view.options().is_some() {
                            view.group = None;
                            cx.notify();
                        }
                    })
                })
                .checked(selected.is_none()),
            );
            if let Some(summary) = &summary {
                for group in &summary.groups {
                    let name = group.name.clone();
                    let label = if name.is_empty() { text!(browse::ungrouped).to_owned() } else { name.replace('/', " / ") };
                    let label = format!(text!(browse::import_group_count), group.counts.total(), label = label);
                    let checked = selected.as_ref() == Some(&name);
                    let view = view.clone();
                    menu = menu.item(
                        components::menu_item(label, move |_, _, cx| {
                            view.update(cx, |view, cx| {
                                if view.options().is_some() {
                                    view.group = Some(name.clone());
                                    cx.notify();
                                }
                            })
                        })
                        .checked(checked),
                    );
                }
            }
            menu
        });
        let counts = self.counts();
        let mut body = div().flex().flex_col().gap(px(components::SPACE_MD));
        if self.source == ReaderImportSource::Readest {
            body = body.child(div().flex().items_center().gap(px(components::SPACE_SM)).child(text!(browse::group)).child(div().flex_1().min_w_0().child(selector)));
        }
        if self.loading {
            body = body.child(components::form_hint(text!(common::loading), theme));
        } else if self.summary.is_some() {
            body = body.child(components::form_hint(
                SharedString::from(format!(text!(browse::selected_epub_pdf), counts.total(), if counts.total() == 1 { text!(browse::book) } else { text!(browse::books_plural) }, counts.epub, counts.pdf)),
                theme,
            ));
        }
        body = body.child(
            div()
                .id("reader-file-modes")
                .role(Role::RadioGroup)
                .aria_label(text!(browse::book_files))
                .flex()
                .flex_wrap()
                .gap(px(components::SPACE_MD))
                .child(self.mode_choice(ReaderImportMode::Copy, text!(browse::copy_files), locked, window, cx))
                .child(self.mode_choice(ReaderImportMode::Move, text!(browse::move_files), locked || !self.summary.as_ref().is_some_and(|summary| summary.can_move), window, cx)),
        );
        if self.importing {
            body = body.child(components::form_hint(text!(browse::importing_books), theme));
        }
        if let Some(error) = &self.error {
            body = body.child(components::error_text(theme).child(error.clone()));
        }
        let count = counts.total();
        let buttons = div().flex().justify_end().gap(px(components::SPACE_SM)).child(components::dialog_cancel_button("reader-close-dialog", text!(common::close)).on_click(cx.listener(|view, _, _, cx| view.dismiss(cx)))).child(
            components::base_button("reader-import")
                .primary()
                .label(format!(text!(browse::import), if count == 1 { text!(browse::book) } else { text!(browse::books_plural) }, count = count))
                .disabled(self.options().is_none())
                .on_click(cx.listener(|view, _, _, cx| view.start_import(cx))),
        );
        div()
            .track_focus(&self.focus)
            .key_context("ReaderImport")
            .on_action(cx.listener(|view, _: &crate::DismissSettings, _, cx| {
                view.dismiss(cx);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => view.dismiss(cx),
                    "enter" => view.start_import(cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .w_full()
            .min_w_0()
            .min_h_0()
            .max_h_full()
            .flex()
            .flex_col()
            .text_color(theme.text)
            .bg(theme.page_bg)
            .child(div().flex_none().p(px(components::SPACE_MD)).child(heading))
            .child(div().id("reader-import-body").min_h_0().overflow_y_scrollbar().px(px(components::SPACE_MD)).pb(px(components::SPACE_MD)).child(body))
            .child(div().flex_none().p(px(components::SPACE_MD)).border_t_1().border_color(theme.rule).child(buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use library_backend::ReaderGroup;
    use sync_common::ROOT_DIR_ID;

    #[gpui::test]
    async fn group_selection_keeps_the_implicit_destination_and_copy_default(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, context, _app_client) = crate::navigation::LibrarySession::test_context(cx).await;
        let library = context.backend().clone();
        library.create_directory(ROOT_DIR_ID, "Destination".into()).await.unwrap();
        let parent = library.folder_destinations().await.unwrap().into_iter().find(|folder| folder.label == "Destination").unwrap().id;
        let summary = ReaderLibrarySummary { counts: ReaderBookCounts { epub: 3, pdf: 2 }, groups: vec![ReaderGroup { name: "Dator/Programmering".into(), counts: ReaderBookCounts { epub: 2, pdf: 0 } }], can_move: true };
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| ReaderImportDialog::with_source(library, ReaderImportSource::Readest, parent, std::future::ready(Ok(summary)), cx));
            gpui_component::Root::new(view, window, cx)
        });
        let view = window.read_with(cx, |root, _| root.view().clone().downcast::<ReaderImportDialog>().ok().unwrap()).unwrap();
        cx.condition(&view, |view, _| !view.loading).await;
        view.update(cx, |view, _| {
            assert_eq!(view.counts().total(), 5);
            assert_eq!(view.options().unwrap().parent_id, parent);
            assert_eq!(view.options().unwrap().mode, ReaderImportMode::Copy);
            assert_eq!(view.options().unwrap().group, None);
            view.group = Some("Dator/Programmering".into());
            assert_eq!(view.counts().total(), 2);
            assert_eq!(view.options().unwrap().parent_id, parent);
            assert_eq!(view.options().unwrap().group.as_deref(), Some("Dator/Programmering"));
        });
    }
    #[gpui::test]
    async fn foliate_uses_the_same_destination_and_copy_default_without_groups(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, context, _app_client) = crate::navigation::LibrarySession::test_context(cx).await;
        let library = context.backend().clone();
        let summary = ReaderLibrarySummary { counts: ReaderBookCounts { epub: 4, pdf: 1 }, groups: Vec::new(), can_move: true };
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| ReaderImportDialog::with_source(library, ReaderImportSource::Foliate, ROOT_DIR_ID, std::future::ready(Ok(summary)), cx));
            gpui_component::Root::new(view, window, cx)
        });
        let view = window.read_with(cx, |root, _| root.view().clone().downcast::<ReaderImportDialog>().ok().unwrap()).unwrap();
        cx.condition(&view, |view, _| !view.loading).await;
        view.update(cx, |view, _| {
            assert_eq!(view.source, ReaderImportSource::Foliate);
            assert_eq!(view.counts().total(), 5);
            let options = view.options().unwrap();
            assert_eq!(options.parent_id, ROOT_DIR_ID);
            assert_eq!(options.mode, ReaderImportMode::Copy);
            assert!(options.group.is_none());
            assert!(view.summary.as_ref().unwrap().groups.is_empty());
        });
    }
}
