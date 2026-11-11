//! Import and order device-local dictionaries without involving a library.
use ui_text::text;

use std::{collections::HashSet, rc::Rc};

use dictionary::{Client, DictionaryDownloads, DictionaryImport, DictionaryInfo, DictionarySource, OperationError, OperationResult as Result};
use dictionary_catalog::{CuratedDictionary, find};
use gpui::prelude::*;
use gpui::{App, Context, Entity, EventEmitter, FileDialogFilter, FocusHandle, IntoElement, MouseButton, PathPromptOptions, Render, SharedString, Task, WeakEntity, Window, div, px, rems};
use gpui_component::checkbox::Checkbox;
use gpui_component::list::{List, ListDelegate, ListEvent, ListItem, ListState};
use gpui_component::{Disableable, IconName, IndexPath};
use ui_components as components;
use ui_components::{Classified, Failure};

fn error_message(error: &OperationError, fallback: &'static str) -> &'static str {
    if error.failure() == Some(Failure::StorageFull) {
        text!(dictionary::disk_full)
    } else {
        components::user_facing_error(error, fallback)
    }
}

struct DictionaryService(Rc<dyn Client>);
impl gpui::Global for DictionaryService {}

pub fn configure(client: Rc<dyn Client>, cx: &mut gpui::App) {
    cx.set_global(DictionaryService(client));
}

pub fn client(cx: &gpui::App) -> Rc<dyn Client> {
    cx.global::<DictionaryService>().0.clone()
}

pub struct Closed;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Import,
    Download,
}

pub struct DictionarySettings {
    backend: Rc<dyn Client>,
    dictionaries: Vec<DictionaryInfo>,
    operation: Option<Operation>,
    pending: HashSet<String>,
    error: Option<SharedString>,
    snapshot_failed: bool,
    loading: bool,
    languages: Vec<String>,
    downloads: Option<DictionaryDownloads>,
    picker: Option<Entity<ListState<DictionaryList>>>,
    download_details: Option<String>,
    picker_focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    details: Option<String>,
    closable: bool,
    _changes: Task<()>,
}

impl EventEmitter<Closed> for DictionarySettings {}

impl DictionarySettings {
    pub fn closable(mut self) -> Self {
        self.closable = true;
        self
    }

    pub fn new(backend: Rc<dyn Client>, mut languages: Vec<String>, cx: &mut Context<Self>) -> Self {
        languages.extend(sys_locale::get_locales());
        let mut view = Self {
            backend,
            dictionaries: Vec::new(),
            operation: None,
            pending: HashSet::new(),
            error: None,
            snapshot_failed: false,
            loading: false,
            languages,
            downloads: None,
            picker: None,
            download_details: None,
            picker_focus: cx.focus_handle(),
            previous_focus: None,
            details: None,
            closable: false,
            _changes: Task::ready(()),
        };
        view.reload(cx);
        view
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.snapshot_failed = false;
        let client = self.backend.clone();
        let languages = self.languages.clone();
        self._changes = cx.spawn(async move |this, cx| {
            let mut changes = match client.changes().await {
                Ok(changes) => changes,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.load_failed(&error, cx));
                    return;
                }
            };
            loop {
                let snapshot = client.snapshot(languages.clone()).await;
                if this
                    .update(cx, |this, cx| {
                        this.loading = false;
                        match snapshot {
                            Ok(snapshot) => {
                                this.dictionaries = snapshot.dictionaries;
                                this.downloads = Some(snapshot.downloads);
                                this.snapshot_failed = false;
                            }
                            Err(error) => return this.load_failed(&error, cx),
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                if let Err(error) = changes.changed().await {
                    let error = OperationError::from_error(&error);
                    let _ = this.update(cx, |this, cx| this.load_failed(&error, cx));
                    return;
                }
            }
        });
        cx.notify();
    }

    fn load_failed(&mut self, error: &OperationError, cx: &mut Context<Self>) {
        log::warn!("Could not load dictionaries: {error}");
        self.loading = false;
        self.snapshot_failed = true;
        self.error = Some(error_message(error, text!(dictionary::load_failed)).into());
        cx.notify();
    }

    fn finish(&mut self, result: Result<()>, cx: &mut Context<Self>) {
        // The change subscription owns snapshots; an older operation result
        // must not overwrite a newer change notification.
        self.error = result.err().map(|error| {
            log::warn!("Dictionary operation failed: {error}");
            error_message(&error, text!(dictionary::update_failed)).into()
        });
        cx.notify();
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        if self.operation.is_some() {
            return;
        }
        let selection = cx.prompt_for_files_with_filters(
            PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(text!(dictionary::import_stardict_dictionary).into()) },
            vec![FileDialogFilter { name: text!(dictionary::dictionary_files_or_zip).into(), extensions: ["ifo", "idx", "gz", "dict", "dz", "syn", "search", "data", "json", "zip"].map(str::to_owned).to_vec() }],
        );
        self.operation = Some(Operation::Import);
        self.error = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let Some(selected) = selection.await.map_err(OperationError::operation)?.map_err(OperationError::operation)? else {
                    return Ok(());
                };
                let sources = selected
                    .into_iter()
                    .map(|file| {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            DictionarySource::new(file.name, file.source.size_bytes, move || Box::pin(async move { file.source.open().await.map_err(OperationError::operation) }))
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            let mut source = file.source;
                            DictionarySource::from_browser(file.name, source.size_bytes, source.take_browser_file())
                        }
                    })
                    .collect();
                backend.import(DictionaryImport::new(sources)?).await.map(|_| ())
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.operation = None;
                this.finish(result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn configure(&mut self, id: String, enabled: bool, direction: i8, cx: &mut Context<Self>) {
        if self.operation == Some(Operation::Import) || !self.pending.insert(id.clone()) {
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = backend.configure(id.clone(), enabled, direction).await.map(|_| ());
            let _ = this.update(cx, |this, cx| {
                this.pending.remove(&id);
                this.finish(result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        if self.operation == Some(Operation::Import) || !self.pending.insert(id.clone()) {
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = backend.remove(id.clone()).await.map(|_| ());
            let _ = this.update(cx, |this, cx| {
                this.pending.remove(&id);
                this.finish(result, cx);
            });
        })
        .detach();
        cx.notify();
    }
}

impl DictionarySettings {
    fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(downloads) = &self.downloads else { return };
        let mut available = downloads.available.clone();
        available.sort_by_key(|spec| downloads.recommended.iter().position(|id| id == &spec.id).unwrap_or(usize::MAX));
        let owner = cx.entity();
        let list = cx.new(|cx| {
            cx.observe(&owner, |_, _, cx| cx.notify()).detach();
            ListState::new(DictionaryList { owner: owner.downgrade(), filtered: (0..available.len()).collect(), available }, window, cx).searchable(true)
        });
        cx.subscribe_in(&list, window, |this, list, event: &ListEvent, window, cx| {
            if let ListEvent::Confirm(index) = event {
                let delegate = list.read(cx).delegate();
                let id = delegate.filtered.get(index.row).and_then(|index| delegate.available.get(*index)).map(|spec| spec.id.clone());
                if let Some(id) = id {
                    this.download(id, window, cx);
                }
            }
        })
        .detach();
        self.previous_focus = window.focused(cx);
        list.update(cx, |list, cx| list.focus(window, cx));
        self.picker = Some(list);
        self.download_details = None;
        cx.notify();
    }

    fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    fn download(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation.is_some() || self.downloads.as_ref().is_some_and(|downloads| downloads.active.is_some()) || self.dictionaries.iter().any(|dictionary| dictionary.curated_id.as_deref() == Some(id.as_str())) {
            return;
        }
        self.close_picker(window, cx);
        self.operation = Some(Operation::Download);
        self.error = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = backend.download(id).await.map(|_| ());
            let _ = this.update(cx, |this, cx| {
                this.operation = None;
                this.finish(result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn render_picker(&self, window: &Window, cx: &mut Context<Self>) -> Option<gpui::Deferred> {
        let list = self.picker.as_ref()?;
        let downloads = self.downloads.as_ref()?;
        let theme = components::browser_theme(cx);
        let owner = cx.weak_entity();
        let content = List::new(list).search_placeholder(text!(dictionary::filter_dictionaries)).search_prefix(move |_, _| {
            let owner = owner.clone();
            components::back_text_button("dictionary-picker-back", text!(common::back))
                .on_click(move |_, window, cx| { let _ = owner.update(cx, |settings, cx| settings.close_picker(window, cx)); }).into_any_element()
        });
        let details = self.download_details.as_deref().and_then(|id| downloads.available.iter().find(|spec| spec.id == id)).map(|spec| {
            let source_url = spec.source_url.clone();
            let source = components::outlined_text_button(format!("dictionary-source-link-{}", spec.id), text!(dictionary::source)).ghost().on_click(move |_, _, cx| cx.open_url(&source_url));
            let mut links = div().flex().flex_wrap().gap(px(8.0)).child(source);
            if let Some(version) = spec.license.strip_prefix("CC-BY-SA-") {
                let license_url = format!("https://creativecommons.org/licenses/by-sa/{version}/");
                links = links.child(components::outlined_text_button(format!("dictionary-license-link-{}", spec.id), text!(dictionary::license)).ghost().on_click(move |_, _, cx| cx.open_url(&license_url)));
            }
            div()
                .flex_none()
                .p(px(12.0))
                .bg(theme.hover)
                .flex()
                .flex_col()
                .gap(px(4.0))
                .text_size(gpui::rems(components::TEXT_XS))
                .child(text!(common::source_and_license))
                .child(spec.source.clone())
                .child(div().text_color(theme.text_muted).child(format!(text!(dictionary::edition), spec.version, spec.license)))
                .child(links)
        });
        let height = (f32::from(window.viewport_size().height) - 32.0).clamp(1.0, 640.0);
        let panel = (if components::touch_ui() { components::bottom_sheet_surface(theme) } else { components::modal_form_surface(theme) })
            .p_0()
            .gap_0()
            .overflow_hidden()
            .w(px(640.0))
            .h(px(height))
            .when(components::touch_ui(), |panel| {
                let rows = list.read(cx).delegate().filtered.len().max(1);
                let safe = window.insets().effective();
                let available = window.viewport_size().height - safe.top - safe.bottom;
                panel.w_full().h((px(48.0) * rows as f32 + window.rem_size() * components::settings_control_size_rem()).min(available * 0.8))
            })
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| {
                this.close_picker(window, cx);
                cx.stop_propagation();
            }))
            .track_focus(&self.picker_focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_picker(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(div().id("dictionary-picker-list").flex_1().min_h_0().child(content))
            .children(details);
        let panel = components::contain_modal_focus(panel, &self.picker_focus, "Dialog ModalFocus", cx);
        let scrim = components::modal_scrim("dictionary-picker-scrim", theme)
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| {
                this.close_picker(window, cx);
                cx.stop_propagation();
            }))
            .on_click(cx.listener(|this, _, window, cx| this.close_picker(window, cx)));
        let overlay = if components::touch_ui() { components::anchor_to_window(components::bottom_sheet_overlay().child(scrim).child(panel), window) } else { components::centered_modal(scrim, panel, window) };
        Some(gpui::deferred(overlay).with_priority(20))
    }
}

struct DictionaryList {
    owner: WeakEntity<DictionarySettings>,
    available: Vec<CuratedDictionary>,
    filtered: Vec<usize>,
}

impl ListDelegate for DictionaryList {
    type Item = ListItem;

    fn perform_search(&mut self, query: &str, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Task<()> {
        let query = query.trim().to_lowercase();
        self.filtered = self.available.iter().enumerate().filter_map(|(index, spec)| spec.name.to_lowercase().contains(&query).then_some(index)).collect();
        cx.notify();
        Task::ready(())
    }


    fn items_count(&self, _: usize, _: &App) -> usize {
        self.filtered.len()
    }

    fn render_item(&mut self, index: IndexPath, _: &mut Window, cx: &mut Context<ListState<Self>>) -> Option<ListItem> {
        let spec = self.available.get(*self.filtered.get(index.row)?)?;
        let owner = self.owner.upgrade()?;
        let settings = owner.read(cx);
        let downloads = settings.downloads.as_ref()?;
        let installed = settings.dictionaries.iter().any(|item| item.curated_id.as_deref() == Some(spec.id.as_str()));
        let busy = settings.operation.is_some() || downloads.active.is_some();
        let theme = components::browser_theme(cx);
        let id = spec.id.clone();
        let info_owner = self.owner.clone();
        let info = components::outlined_icon_button(format!("dictionary-source-{id}"), format!(text!(dictionary::source_and_license_for), spec.name), IconName::Info).on_click(move |_, _, cx| {
            cx.stop_propagation();
            let _ = info_owner.update(cx, |settings, cx| {
                settings.download_details = if settings.download_details.as_ref() == Some(&id) { None } else { Some(id.clone()) };
                cx.notify();
            });
        });
        let id = spec.id.clone();
        let download_owner = self.owner.clone();
        let download = components::outlined_text_button(format!("dictionary-download-{id}"), if installed { text!(dictionary::installed) } else { text!(common::download) }).disabled(installed || busy).on_click(move |_, window, cx| {
            cx.stop_propagation();
            let _ = download_owner.update(cx, |settings, cx| settings.download(id.clone(), window, cx));
        });
        let size = div().flex_none().text_size(rems(components::TEXT_XS)).text_color(theme.text_muted).child(format!(text!(dictionary::mib), spec.download_bytes() as f64 / (1024.0 * 1024.0)));
        Some(
            ListItem::new(spec.id.clone()).h(rems(components::settings_control_size_rem())).when(components::touch_ui(), |row| row.h(px(48.0)).px(px(components::SPACE_MD))).border_b_1().border_color(theme.rule).child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(rems(0.5))
                    .child(div().flex_1().min_w_0().text_ellipsis().child(spec.name.clone()))
                    .child(size)
                    .child(div().flex_none().child(info))
                    .child(div().flex_none().child(download)),
            ),
        )
    }

    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}
}

impl Render for DictionarySettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let import = components::outlined_text_button("import-stardict", text!(dictionary::import)).debug_selector(|| "import-stardict".into()).disabled(self.operation.is_some()).on_click(cx.listener(|this, _, _, cx| this.import(cx)));
        let download = components::outlined_text_button("download-dictionary", text!(dictionary::download_dictionary))
            .debug_selector(|| "download-dictionary".into())
            .disabled(self.operation == Some(Operation::Import) || self.downloads.is_none())
            .on_click(cx.listener(|this, _, window, cx| this.open_picker(window, cx)));
        let header = div().w_full().min_w_0().flex().items_center().gap(px(components::SPACE_SM))
            .child(components::back_text_button("dictionary-settings-back", text!(common::back)).on_click(cx.listener(|_, _, _, cx| cx.emit(Closed))))
            .child(div().flex_1().min_w_0().child(text!(dictionary::dictionaries)));
        let actions = div().flex().flex_wrap().items_center().gap(px(8.0)).py(px(12.0)).child(import).child(download);
        let mut group = if self.closable { components::browser_settings_group().child(header) } else { components::browser_settings_fieldset(text!(dictionary::dictionaries), theme) }.child(actions);
        if let Some(downloads) = &self.downloads {
            if let Some(active) = downloads.active.as_deref().and_then(|id| downloads.available.iter().find(|spec| spec.id == id)) {
                group = group.child(div().p(px(12.0)).child(format!(text!(dictionary::downloading_dictionary), active.name)));
            }
        }
        if self.loading {
            group = group.child(components::form_hint(text!(common::loading), theme));
        } else if self.dictionaries.is_empty() && !self.snapshot_failed && self.operation.is_none() {
            group = group.child(div().py(px(12.0)).text_color(theme.text_muted).child(text!(dictionary::no_dictionaries_installed)));
        }
        for (index, dictionary) in self.dictionaries.iter().enumerate() {
            let id = dictionary.id.clone();
            let enabled = dictionary.enabled;
            let busy = self.operation == Some(Operation::Import) || self.pending.contains(&id);
            let toggle = Checkbox::new(SharedString::from(format!("dictionary-toggle-{id}")))
                .tooltip(text!(dictionary::enabled))
                .checked(enabled)
                .disabled(busy)
                .on_click(cx.listener(move |this, checked, _, cx| this.configure(id.clone(), *checked, 0, cx)));
            let id = dictionary.id.clone();
            let info = components::outlined_icon_button(format!("dictionary-details-{id}"), text!(dictionary::dictionary_details), IconName::Info).no_fill().on_click(cx.listener(move |this, _, _, cx| {
                this.details = if this.details.as_ref() == Some(&id) { None } else { Some(id.clone()) };
                cx.notify();
            }));
            let mut controls = div().flex().items_center().flex_wrap().gap(px(4.0)).child(toggle).child(info);
            let id = dictionary.id.clone();
            controls = controls.child(
                components::outlined_icon_button(format!("dictionary-remove-{id}"), text!(dictionary::remove_dictionary), components::navigation_icon(components::NavigationIcon::Trash2))
                    .no_fill()
                    .text_color(theme.danger)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| this.remove(id.clone(), cx))),
            );
            for (direction, label, icon, disabled) in [(-1, text!(dictionary::move_up), IconName::ArrowUp, index == 0), (1, text!(dictionary::move_down), IconName::ArrowDown, index + 1 == self.dictionaries.len())] {
                let id = dictionary.id.clone();
                controls = controls.child(
                    components::outlined_icon_button(format!("dictionary-move-{id}-{direction}"), label, icon)
                        .no_fill()
                        .disabled(busy || disabled)
                        .on_click(cx.listener(move |this, _, _, cx| this.configure(id.clone(), enabled, direction, cx))),
                );
            }
            let details = format!(text!(dictionary::words_mib), dictionary.metadata.word_count, dictionary.size_bytes as f64 / (1024.0 * 1024.0));
            let label = dictionary.curated_id.as_deref().and_then(find).map_or_else(|| dictionary.metadata.name.clone(), |spec| spec.name.clone());
            group = group.child(components::browser_settings_row(label, details, controls, theme));
            if self.details.as_deref() == Some(dictionary.id.as_str()) {
                group = group.child(div().p(px(12.0)).child(format!("{}\n\n{}", dictionary.metadata.name, dictionary.metadata.description)));
            }
        }
        let error = self
            .error
            .clone()
            .or_else(|| self.snapshot_failed.then(|| text!(dictionary::load_failed).into()))
            .or_else(|| self.downloads.as_ref().and_then(|downloads| downloads.error.as_ref()).map(|error| error_message(error, text!(dictionary::download_failed)).into()));
        group
            .children(error.map(|error| components::browser_settings_group_error(error, theme)))
            .when(self.snapshot_failed, |group| group.child(components::outlined_text_button("dictionary-retry", text!(common::retry)).disabled(self.loading).on_click(cx.listener(|this, _, _, cx| this.reload(cx)))))
            .children(self.render_picker(window, cx))
    }
}
