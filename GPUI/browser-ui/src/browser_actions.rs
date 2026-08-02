use bokheim_ui_components as components;
use epub_svelte_lib::frontend_contract::FrontendContract;
use epub_svelte_lib::preferences::{BrowsingPreferencePatch, BrowsingPreferences};
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FileDialogFilter, PathPromptOptions, Window, px};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::Input;
use gpui_component::{Theme, ThemeTokens, WindowExt};

use crate::BrowserView;

pub(crate) fn apply_browsing_appearance(preferences: BrowsingPreferences, cx: &mut App) {
    let palette = preferences.theme().palette();
    let app_bg: gpui::Hsla = gpui::rgb(palette.app_bg).into();
    let surface: gpui::Hsla = gpui::rgb(palette.surface_bg).into();
    let card: gpui::Hsla = gpui::rgb(palette.card_bg).into();
    let text: gpui::Hsla = gpui::rgb(palette.ink_text).into();
    let muted_text: gpui::Hsla = gpui::rgb(palette.subtle_text).into();
    let accent: gpui::Hsla = gpui::rgb(palette.accent).into();
    let border: gpui::Hsla = gpui::rgb(palette.border).into();
    let hover: gpui::Hsla = gpui::rgb(palette.hover_bg).into();
    let active: gpui::Hsla = gpui::rgb(palette.active_highlight).into();
    let white_surface: gpui::Hsla = gpui::rgb(palette.white_surface).into();
    let danger: gpui::Hsla = gpui::rgb(palette.danger_text).into();
    let danger_bg: gpui::Hsla = gpui::rgb(palette.danger_bg).into();

    let application_theme = Theme::global_mut(cx);
    application_theme.font_family = preferences.interface_font().font_name().into();
    application_theme.font_size = px(preferences.ui_font_size_px() as f32);
    let colors = &mut application_theme.colors;
    colors.background = app_bg;
    colors.foreground = text;
    colors.border = border;
    colors.accent = active;
    colors.accent_foreground = text;
    colors.group_box = card;
    colors.group_box_foreground = text;
    colors.input = border;
    colors.muted = hover;
    colors.muted_foreground = muted_text;
    colors.popover = white_surface;
    colors.popover_foreground = text;
    colors.primary = accent;
    colors.primary_active = accent;
    colors.primary_hover = accent;
    colors.primary_foreground = white_surface;
    colors.secondary = white_surface;
    colors.secondary_active = active;
    colors.secondary_hover = hover;
    colors.secondary_foreground = text;
    colors.button = white_surface;
    colors.button_active = active;
    colors.button_hover = hover;
    colors.button_foreground = text;
    colors.button_primary = accent;
    colors.button_primary_active = accent;
    colors.button_primary_hover = accent;
    colors.button_primary_foreground = white_surface;
    colors.button_secondary = white_surface;
    colors.button_secondary_active = active;
    colors.button_secondary_hover = hover;
    colors.button_secondary_foreground = text;
    colors.caret = accent;
    colors.list = surface;
    colors.list_active = active;
    colors.list_active_border = border;
    colors.list_even = surface;
    colors.list_head = surface;
    colors.list_hover = hover;
    colors.selection = active;
    colors.sidebar = app_bg;
    colors.sidebar_accent = active;
    colors.sidebar_accent_foreground = accent;
    colors.sidebar_border = border;
    colors.sidebar_foreground = text;
    colors.sidebar_primary = accent;
    colors.sidebar_primary_foreground = white_surface;
    colors.title_bar = surface;
    colors.title_bar_border = border;
    colors.danger = danger_bg;
    colors.danger_foreground = danger;
    colors.ring = accent;
    application_theme.tokens = ThemeTokens::from(application_theme.colors);
    cx.refresh_windows();
}

impl BrowserView {
    pub(crate) fn render_browsing_settings(&self, target: Entity<Self>, theme: components::BrowserTheme, max_height: f32) -> AnyElement {
        let contract = FrontendContract::default().browsing_preferences;
        let preferences = self.browsing_preferences;
        let heading_font = self.heading_font_name();
        let current_theme = preferences.theme();
        let current_interface_font = preferences.interface_font();
        let current_heading_font = preferences.heading_font();
        let current_font_size = preferences.ui_font_size_px();
        let [minimum_font_size, maximum_font_size] = contract.constraints.ui_font_size_px;

        let mut application_themes = components::browser_settings_fieldset("Application theme", theme);
        for (index, option) in contract.options.themes.iter().enumerate() {
            let value = option.value;
            let target = target.clone();
            application_themes = application_themes.child(
                components::browser_settings_choice_card(format!("application-theme-{index}"), option.label.clone(), option.description.clone(), value == current_theme, heading_font.clone(), theme).on_click(move |checked, _, cx| {
                    if *checked {
                        target.update(cx, |this, cx| {
                            this.update_browsing_preference(BrowsingPreferencePatch::Theme(value), cx);
                        });
                    }
                }),
            );
        }

        let mut interface_fonts = components::browser_settings_font_choices(theme);
        for (index, option) in contract.options.fonts.iter().enumerate() {
            let value = option.value;
            let target = target.clone();
            let checked = value == current_interface_font;
            interface_fonts = interface_fonts.child(components::browser_settings_font_button(format!("interface-font-{index}"), option.font_name.clone(), checked, option.font_name.clone(), theme).on_click(move |_, _, cx| {
                target.update(cx, |this, cx| {
                    this.update_browsing_preference(BrowsingPreferencePatch::InterfaceFont(value), cx);
                });
            }));
        }

        let mut heading_fonts = components::browser_settings_font_choices(theme);
        for (index, option) in contract.options.fonts.iter().enumerate() {
            let value = option.value;
            let target = target.clone();
            let checked = value == current_heading_font;
            heading_fonts = heading_fonts.child(components::browser_settings_font_button(format!("heading-font-{index}"), option.font_name.clone(), checked, option.font_name.clone(), theme).on_click(move |_, _, cx| {
                target.update(cx, |this, cx| {
                    this.update_browsing_preference(BrowsingPreferencePatch::HeadingFont(value), cx);
                });
            }));
        }

        let decrease_target = target.clone();
        let decrease_font_size = current_font_size.saturating_sub(1).max(minimum_font_size);
        let decrease = components::browser_settings_font_size_button("decrease-ui-font-size", "−", current_font_size == minimum_font_size, theme).on_click(move |_, _, cx| {
            decrease_target.update(cx, |this, cx| {
                this.update_browsing_preference(BrowsingPreferencePatch::UiFontSizePx(decrease_font_size), cx);
            });
        });
        let increase_target = target.clone();
        let increase_font_size = current_font_size.saturating_add(1).min(maximum_font_size);
        let increase = components::browser_settings_font_size_button("increase-ui-font-size", "+", current_font_size == maximum_font_size, theme).on_click(move |_, _, cx| {
            increase_target.update(cx, |this, cx| {
                this.update_browsing_preference(BrowsingPreferencePatch::UiFontSizePx(increase_font_size), cx);
            });
        });
        let font_size = components::browser_settings_font_size_stepper(decrease, format!("{current_font_size}px"), increase, theme);

        let sections = components::browser_settings_sections()
            .child(application_themes)
            .child(components::browser_settings_font_row("Interface font", interface_fonts, theme))
            .child(components::browser_settings_font_row("Heading font", heading_fonts, theme))
            .child(components::browser_settings_font_row("UI font size", font_size, theme));
        let close_target = target.clone();
        let settings_header =
            components::browser_settings_header_row().child(components::browser_settings_header("Appearance", "Browsing settings", heading_font, theme)).child(components::browser_settings_close_button(theme).on_click(move |_, _, cx| {
                close_target.update(cx, |this, cx| {
                    this.settings_open = false;
                    cx.notify();
                });
            }));
        let settings_card = components::browser_settings_card(theme).child(settings_header).child(sections);
        let scrim_target = target;
        components::browser_settings_overlay()
            .child(components::browser_settings_scrim(theme).on_click(move |_, _, cx| {
                scrim_target.update(cx, |this, cx| {
                    this.settings_open = false;
                    cx.notify();
                });
            }))
            .child(components::browser_settings_dialog(px(max_height)).child(settings_card))
            .into_any_element()
    }

    fn update_browsing_preference(&mut self, patch: BrowsingPreferencePatch, cx: &mut Context<Self>) {
        match self.app_core.update_browsing_preference(patch) {
            Ok(preferences) => {
                self.browsing_preferences = preferences;
                apply_browsing_appearance(preferences, cx);
                cx.notify();
            }
            Err(error) => log::error!("failed to save browsing preferences: {error}"),
        }
    }

    pub(crate) fn choose_book_to_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(library) = self.active_library.clone() else {
            return;
        };
        let folder_id = self.active_folder_id();
        let paths = cx.prompt_for_paths_with_filters(
            PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add book".into()) },
            vec![FileDialogFilter { name: "EPUB and PDF books".to_owned(), extensions: vec!["epub".to_owned(), "pdf".to_owned()] }],
        );
        let worker = self.blocking_worker.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()).map(str::to_owned) else {
                log::error!("selected book path has no valid file name: {}", path.display());
                return;
            };
            let import = worker.dispatch(move || library.import_book(&folder_id, &path, &file_name).map(|_| ()).map_err(|error| error.to_string()));
            let Ok(result) = import.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.refresh_active_section(cx),
                Err(error) => log::error!("failed to add book: {error}"),
            });
        })
        .detach();
    }

    pub(crate) fn show_new_folder_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_folder_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        let input = self.new_folder_input.clone();
        let target = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let input_for_submit = input.clone();
            let target = target.clone();
            dialog.title("New folder").child(Input::new(&input)).button_props(DialogButtonProps::default().ok_text("Create").show_cancel(true)).on_ok(move |_, _, cx: &mut App| {
                let name = input_for_submit.read(cx).value().trim().to_owned();
                if name.is_empty() {
                    return false;
                }
                target.update(cx, |this, cx| {
                    this.create_folder(name.clone(), cx);
                });
                true
            })
        });
    }

    fn create_folder(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(library) = self.active_library.clone() else {
            return;
        };
        let folder_id = self.active_folder_id();
        let create = self.blocking_worker.dispatch(move || library.create_directory(&folder_id, &name).map(|_| ()).map_err(|error| error.to_string()));
        cx.spawn(async move |this, cx| {
            let Ok(result) = create.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.refresh_active_section(cx),
                Err(error) => log::error!("failed to create folder: {error}"),
            });
        })
        .detach();
    }
}
