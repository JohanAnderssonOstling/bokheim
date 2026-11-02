//! Import and order device-local dictionaries without involving a library.
use ui_text::text;

use std::{collections::HashSet, rc::Rc};

use dictionary::{Client, DictionaryDownloads, DictionaryImport, DictionaryInfo, DictionarySource, Result};
use dictionary_catalog::{find, for_language};
use gpui::prelude::*;
use gpui::{Context, EventEmitter, FileDialogFilter, FocusHandle, IntoElement, PathPromptOptions, Render, SharedString, Task, Window, div, px};
use gpui_component::checkbox::Checkbox;
use gpui_component::{Disableable, IconName};
use ui_components as components;
use ui_components::ScrollableElement;

struct DictionaryService(Rc<dyn Client>);
impl gpui::Global for DictionaryService {}

pub fn configure(client: Rc<dyn Client>, cx: &mut gpui::App) {
    cx.set_global(DictionaryService(client));
}

pub fn client(cx: &gpui::App) -> Rc<dyn Client> {
    cx.global::<DictionaryService>().0.clone()
}

pub struct Closed;

pub struct DictionarySettings {
    backend: Rc<dyn Client>,
    dictionaries: Vec<DictionaryInfo>,
    busy: bool,
    pending: HashSet<String>,
    error: Option<SharedString>,
    downloads: Option<DictionaryDownloads>,
    picker_open: bool,
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
        let client = backend.clone();
        let changes = cx.spawn(async move |this, cx| {
            let mut changes = match client.changes().await {
                Ok(changes) => changes,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.finish(Err(error), cx));
                    return;
                }
            };
            loop {
                let snapshot = client.snapshot(languages.clone()).await;
                if this
                    .update(cx, |this, cx| {
                        match snapshot {
                            Ok(snapshot) => {
                                this.dictionaries = snapshot.dictionaries;
                                this.downloads = Some(snapshot.downloads);
                            }
                            Err(error) => this.error = Some(error.to_string().into()),
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
        Self {
            backend,
            dictionaries: Vec::new(),
            busy: false,
            pending: HashSet::new(),
            error: None,
            downloads: None,
            picker_open: false,
            download_details: None,
            picker_focus: cx.focus_handle(),
            previous_focus: None,
            details: None,
            closable: false,
            _changes: changes,
        }
    }

    fn finish(&mut self, result: Result<Vec<DictionaryInfo>>, cx: &mut Context<Self>) {
        match result {
            Ok(dictionaries) => {
                self.dictionaries = dictionaries;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let selection = cx.prompt_for_files_with_filters(
            PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(text!(dictionary::import_stardict_dictionary).into()) },
            vec![FileDialogFilter { name: text!(dictionary::dictionary_files_or_zip).into(), extensions: ["ifo", "idx", "gz", "dict", "dz", "syn", "search", "data", "json", "zip"].map(str::to_owned).to_vec() }],
        );
        self.busy = true;
        self.error = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let Some(selected) = selection.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string())? else {
                    return backend.list().await;
                };
                let sources = selected
                    .into_iter()
                    .map(|file| {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            DictionarySource::new(file.name, file.source.size_bytes, move || Box::pin(async move { file.source.open().await.map_err(|error| error.to_string()) }))
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            let mut source = file.source;
                            DictionarySource::from_browser(file.name, source.size_bytes, source.take_browser_file())
                        }
                    })
                    .collect();
                backend.import(DictionaryImport::new(sources)?).await
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.finish(result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn configure(&mut self, id: String, enabled: bool, direction: i8, cx: &mut Context<Self>) {
        if self.busy || !self.pending.insert(id.clone()) {
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = backend.configure(id.clone(), enabled, direction).await;
            let _ = this.update(cx, |this, cx| {
                this.pending.remove(&id);
                this.finish(result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        if self.busy || !self.pending.insert(id.clone()) {
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = backend.remove(id.clone()).await;
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
        self.previous_focus = window.focused(cx);
        self.picker_focus.focus(window, cx);
        self.picker_open = true;
        self.download_details = None;
        cx.notify();
    }

    fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker_open = false;
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    fn download(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.close_picker(window, cx);
        self.busy = true;
        self.error = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = backend.download(id).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.finish(result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn render_picker(&self, window: &Window, cx: &mut Context<Self>) -> Option<gpui::Deferred> {
        if !self.picker_open {
            return None;
        }
        let downloads = self.downloads.as_ref()?;
        let theme = components::browser_theme(cx);
        let close = components::outlined_text_button("dictionary-picker-close", text!(common::close)).on_click(cx.listener(|this, _, window, cx| this.close_picker(window, cx)));
        let header = div().flex_none().flex().items_center().justify_between().gap(px(12.0)).child(text!(dictionary::download_dictionary)).child(close);
        let mut list = div().id("dictionary-picker-list").flex_1().min_h_0().overflow_y_scrollbar();
        let mut available: Vec<_> = downloads.available.iter().collect();
        available.sort_by_key(|spec| downloads.recommended.iter().position(|id| id == &spec.id).unwrap_or(usize::MAX));
        for spec in available {
            let installed = self.dictionaries.iter().any(|item| item.curated_id.as_deref() == Some(spec.id.as_str()));
            let id = spec.id.clone();
            let info = components::outlined_icon_button(format!("dictionary-source-{id}"), format!(text!(dictionary::source_and_license_for), spec.name), IconName::Info).on_click(cx.listener(move |this, _, _, cx| {
                this.download_details = if this.download_details.as_ref() == Some(&id) { None } else { Some(id.clone()) };
                cx.notify();
            }));
            let id = spec.id.clone();
            let download = components::outlined_text_button(format!("dictionary-download-{id}"), if installed { text!(dictionary::installed) } else { text!(common::download) })
                .disabled(installed || self.busy || downloads.active.is_some())
                .on_click(cx.listener(move |this, _, window, cx| this.download(id.clone(), window, cx)));
            let size = div().text_size(gpui::rems(components::TEXT_XS)).text_color(theme.text_muted).child(format!(text!(dictionary::mib), spec.download_bytes() as f64 / (1024.0 * 1024.0)));
            list = list.child(
                div()
                    .w_full()
                    .min_h(px(48.0))
                    .py(px(8.0))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(12.0))
                    .border_t_1()
                    .border_color(theme.rule)
                    .child(div().flex_1().min_w(px(80.0)).child(spec.name.clone()).when(downloads.recommended.contains(&spec.id), |name| {
                        name.child(
                            div()
                                .debug_selector({
                                    let id = spec.id.clone();
                                    move || format!("dictionary-recommended-{id}")
                                })
                                .text_size(gpui::rems(components::TEXT_XS))
                                .text_color(theme.text_muted)
                                .child(text!(dictionary::recommended)),
                        )
                    }))
                    .child(div().flex_none().flex().items_center().gap(px(8.0)).child(size).child(info).child(download)),
            );
            if self.download_details.as_deref() == Some(spec.id.as_str()) {
                let source_url = spec.source_url.clone();
                let source = components::outlined_text_button(format!("dictionary-source-link-{}", spec.id), text!(dictionary::source)).ghost().on_click(move |_, _, cx| cx.open_url(&source_url));
                let mut links = div().flex().flex_wrap().gap(px(8.0)).child(source);
                if let Some(version) = spec.license.strip_prefix("CC-BY-SA-") {
                    let license_url = format!("https://creativecommons.org/licenses/by-sa/{version}/");
                    links = links.child(components::outlined_text_button(format!("dictionary-license-link-{}", spec.id), text!(dictionary::license)).ghost().on_click(move |_, _, cx| cx.open_url(&license_url)));
                }
                list = list.child(
                    div()
                        .p(px(12.0))
                        .bg(theme.hover)
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .text_size(gpui::rems(components::TEXT_XS))
                        .child(text!(common::source_and_license))
                        .child(spec.source.clone())
                        .child(div().text_color(theme.text_muted).child(format!(text!(dictionary::edition), spec.version, spec.license)))
                        .child(links),
                );
            }
        }
        let height = (f32::from(window.viewport_size().height) - 32.0).clamp(1.0, 640.0);
        let panel = components::modal_form_surface(theme)
            .key_context("Dialog")
            .w(px(640.0))
            .h(px(height))
            .track_focus(&self.picker_focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_picker(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(header)
            .child(list);
        let scrim = components::modal_scrim("dictionary-picker-scrim", theme).on_click(cx.listener(|this, _, window, cx| this.close_picker(window, cx)));
        Some(gpui::deferred(components::centered_modal(scrim, panel, window)).with_priority(20))
    }
}

impl Render for DictionarySettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let import = components::outlined_text_button("import-stardict", text!(dictionary::import)).debug_selector(|| "import-stardict".into()).disabled(self.busy).on_click(cx.listener(|this, _, _, cx| this.import(cx)));
        let download = components::outlined_text_button("download-dictionary", text!(dictionary::download_dictionary))
            .debug_selector(|| "download-dictionary".into())
            .disabled(self.busy || self.downloads.is_none())
            .on_click(cx.listener(|this, _, window, cx| this.open_picker(window, cx)));
        let actions =
            div().flex().items_center().gap(px(8.0)).when(!self.closable, |actions| actions.py(px(12.0))).child(div().flex_1().min_w_0().flex().flex_wrap().gap(px(8.0)).child(import).child(download)).when(self.closable, |actions| {
                actions.child(
                    components::outlined_icon_button("dictionary-settings-close", text!(dictionary::close_dictionary_settings), IconName::Close)
                        .no_fill()
                        .debug_selector(|| "dictionary-settings-close".into())
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Closed))),
                )
            });
        let mut group = if self.closable { components::browser_settings_group() } else { components::browser_settings_fieldset(text!(dictionary::dictionaries), theme) }.child(actions);
        if let Some(downloads) = &self.downloads {
            if let Some(active) = downloads.active.as_deref().and_then(|id| downloads.available.iter().find(|spec| spec.id == id)) {
                group = group.child(div().p(px(12.0)).child(format!(text!(dictionary::downloading_dictionary), active.name)));
            }
            if let Some(error) = &downloads.error {
                group = group.child(components::browser_settings_group_error(error.clone(), theme));
            }
        }
        if self.dictionaries.is_empty() && !self.busy {
            group = group.child(div().py(px(12.0)).text_color(theme.text_muted).child(text!(dictionary::no_dictionaries_installed)));
        }
        for (index, dictionary) in self.dictionaries.iter().enumerate() {
            let id = dictionary.id.clone();
            let enabled = dictionary.enabled;
            let busy = self.busy || self.pending.contains(&id);
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
            let language = dictionary.metadata.language.as_deref().map(str::trim).filter(|language| !language.is_empty());
            let label = dictionary.curated_id.as_deref().and_then(find).or_else(|| language.and_then(for_language)).map_or_else(|| language.unwrap_or(text!(dictionary::unknown_language)).to_owned(), |spec| spec.name.clone());
            let label = if dictionary.curated_id.is_none() { format!(text!(dictionary::imported), label = label) } else { label };
            group = group.child(components::browser_settings_row(label, details, controls, theme));
            if self.details.as_deref() == Some(dictionary.id.as_str()) {
                group = group.child(div().p(px(12.0)).child(format!("{}\n\n{}", dictionary.metadata.name, dictionary.metadata.description)));
            }
        }
        group.children(self.error.clone().map(|error| components::browser_settings_group_error(error, theme))).children(self.render_picker(window, cx))
    }
}
