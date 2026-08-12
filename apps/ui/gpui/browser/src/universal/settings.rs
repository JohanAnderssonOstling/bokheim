//! Appearance, library presentation and transfer policy.
//!
//! Universal: everything it edits is account-scoped rather than library-scoped,
//! so it edits the shared [`Preferences`] store and every library page observes
//! the result.

use epub_svelte_lib::app::preferences::{BrowsingPreferencePatch, TransferPreferencePatch};
use gpui::prelude::*;
use gpui::{div, Context, Entity, IntoElement, Render, SharedString, Window};
use gpui_component::alert::Alert;
use gpui_component::group_box::{GroupBox, GroupBoxVariants};
use gpui_component::switch::Switch;
use ui_components as components;

use crate::services::AppServices;
use crate::stores::Preferences;

pub(crate) struct SettingsPage {
    services: AppServices,
    preferences: Entity<Preferences>,
    error: Option<SharedString>,
    #[cfg(feature = "kobo")]
    kobo_auto_rotate: bool,
}

impl SettingsPage {
    pub(crate) fn new(services: AppServices, preferences: Entity<Preferences>, #[cfg(feature = "kobo")] kobo_auto_rotate: bool, _cx: &mut Context<Self>) -> Self {
        Self {
            services,
            preferences,
            error: None,
            #[cfg(feature = "kobo")]
            kobo_auto_rotate,
        }
    }

    fn update_browsing(&mut self, patch: BrowsingPreferencePatch, cx: &mut Context<Self>) {
        let result = self.preferences.update(cx, |preferences, cx| preferences.update_browsing(patch, cx));
        self.error = result.err().map(|error| SharedString::from(format!("Could not save setting: {error}")));
        cx.notify();
    }

    fn update_transfer(&mut self, patch: TransferPreferencePatch, cx: &mut Context<Self>) {
        let operation = self.preferences.read(cx).transfer_patch(patch);
        let preferences = self.preferences.clone();
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                match result {
                    Ok(updated) => {
                        preferences.update(cx, |preferences, _| preferences.set_transfer(updated));
                        page.error = None;
                    }
                    Err(error) => page.error = Some(format!("Could not save transfer setting: {error}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(feature = "kobo")]
    fn update_kobo_auto_rotate(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.kobo_auto_rotate = enabled;
        (self.services.save_kobo_auto_rotate)(enabled, cx);
        cx.notify();
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let settings = self.services.contracts.settings.clone();
        let contract = self.services.contracts.browsing_preferences.clone();
        let preferences = self.preferences.read(cx).browsing();
        let transfer = self.preferences.read(cx).transfer();
        let entity = cx.entity();

        let current_font_size = preferences.ui_font_size_px();
        let [minimum_font_size, maximum_font_size] = contract.constraints.ui_font_size_px;

        // Light/dark is a separate axis from which theme, but both are compact
        // choices and belong together rather than in tall descriptive cards.
        let mut appearances = components::browser_settings_button_group(theme);
        for (index, option) in contract.options.appearances.iter().enumerate() {
            let value = option.value;
            let entity = entity.clone();
            appearances = appearances.child(components::browser_settings_choice_button(format!("application-appearance-{index}"), option.label.clone(), value == preferences.appearance(), theme).on_click(move |_, _, cx| {
                entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::Appearance(value), cx));
            }));
        }

        let mut application_themes = components::browser_settings_button_group(theme);
        for (index, option) in contract.options.themes.iter().enumerate() {
            let value = option.value;
            let entity = entity.clone();
            application_themes = application_themes.child(components::browser_settings_choice_button(format!("application-theme-{index}"), option.label.clone(), value == preferences.theme(), theme).on_click(move |_, _, cx| {
                entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::Theme(value), cx));
            }));
        }
        let appearance_settings = components::browser_settings_fieldset(settings.appearance_section_label.clone(), theme)
            .child(components::browser_settings_compact_row("Light or dark", appearances, theme))
            .child(components::browser_settings_compact_row(settings.application_theme_label.clone(), application_themes, theme));

        let all_language_values = contract.options.catalog_languages.iter().map(|option| option.value.clone()).collect::<Vec<_>>();
        let all_languages_selected = preferences.included_catalog_languages().is_empty();
        let all_entity = entity.clone();
        let mut catalog_languages =
            components::browser_settings_chip_group().child(components::browser_settings_chip_button("catalog-language-all", settings.all_catalog_languages_label.clone(), all_languages_selected, theme).on_click(move |_, _, cx| {
                all_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::IncludedCatalogLanguages(Vec::new()), cx));
            }));
        for (index, option) in contract.options.catalog_languages.iter().enumerate() {
            let value = option.value.clone();
            let active = all_languages_selected || preferences.included_catalog_languages().contains(&value);
            let mut selected = if all_languages_selected { all_language_values.clone() } else { preferences.included_catalog_languages().to_vec() };
            if active {
                selected.retain(|language| language != &value);
            } else {
                selected.push(value.clone());
                selected.sort();
                selected.dedup();
            }
            if selected.len() == all_language_values.len() {
                selected.clear();
            }
            let language_entity = entity.clone();
            catalog_languages = catalog_languages.child(components::browser_settings_chip_button(format!("catalog-language-{index}"), option.label.clone(), active, theme).on_click(move |_, _, cx| {
                language_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::IncludedCatalogLanguages(selected.clone()), cx));
            }));
        }
        let catalog_language_policy = components::browser_settings_fieldset(settings.catalogues_section_label.clone(), theme)
            .child(div().text_size(gpui::rems(13.0 / 16.0)).font_weight(gpui::FontWeight::MEDIUM).child(settings.included_catalog_languages_label.clone()))
            .child(div().text_size(gpui::rems(11.0 / 16.0)).text_color(theme.text_muted).child(settings.included_catalog_languages_description.clone()))
            .child(catalog_languages);

        let mut interface_fonts = components::browser_settings_font_choices(theme);
        for (index, option) in contract.options.fonts.iter().enumerate() {
            let value = option.value;
            let interface_entity = entity.clone();
            interface_fonts = interface_fonts.child(components::browser_settings_font_button(format!("interface-font-{index}"), option.font_name.clone(), value == preferences.interface_font(), option.font_name.clone(), theme).on_click(
                move |_, _, cx| {
                    interface_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::InterfaceFont(value), cx));
                },
            ));
        }

        let decrease_entity = entity.clone();
        let decrease_size = current_font_size.saturating_sub(1).max(minimum_font_size);
        let decrease = components::browser_settings_font_size_button("decrease-ui-font-size", "−", current_font_size == minimum_font_size, theme).on_click(move |_, _, cx| {
            decrease_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::UiFontSizePx(decrease_size), cx));
        });
        let increase_entity = entity.clone();
        let increase_size = current_font_size.saturating_add(1).min(maximum_font_size);
        let increase = components::browser_settings_font_size_button("increase-ui-font-size", "+", current_font_size == maximum_font_size, theme).on_click(move |_, _, cx| {
            increase_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::UiFontSizePx(increase_size), cx));
        });
        let font_size = components::browser_settings_font_size_stepper(decrease, format!("{current_font_size}px"), increase, theme);

        let network = self.services.backend.network_status();
        let signed_in = self.services.backend.account_status().map(|status| status.signed_in()).unwrap_or(false);
        let server_connected = signed_in && network.connected && network.validated != Some(false);
        let network_label = settings.network_transports.iter().find(|definition| definition.transport == network.transport).map(|definition| definition.label.as_str()).unwrap_or("Network unknown");
        let network_description = format!(
            "{}: {network_label}{}{} · {}",
            settings.current_network_label,
            if network.metered == Some(true) { " · metered" } else { "" },
            if network.validated == Some(false) && network.connected { " · no internet access" } else { "" },
            settings.background_metered_description
        );

        let auto_limit = transfer.auto_download_limit_mb().unwrap_or(0);
        let decrease_auto_entity = entity.clone();
        let decrease_auto = components::browser_settings_font_size_button("decrease-auto-download-limit", "-", auto_limit == 0, theme).on_click(move |_, _, cx| {
            let value = auto_limit.saturating_sub(1);
            decrease_auto_entity.update(cx, |page, cx| page.update_transfer(TransferPreferencePatch::AutoDownloadLimitMb((value > 0).then_some(value)), cx));
        });
        let increase_auto_entity = entity.clone();
        let increase_auto = components::browser_settings_font_size_button("increase-auto-download-limit", "+", auto_limit == u32::MAX, theme).on_click(move |_, _, cx| {
            let value = auto_limit.saturating_add(1);
            increase_auto_entity.update(cx, |page, cx| page.update_transfer(TransferPreferencePatch::AutoDownloadLimitMb(Some(value)), cx));
        });
        let auto_limit_label = if auto_limit == 0 { "Off".to_owned() } else { format!("{auto_limit} MB") };
        let auto_choices = components::browser_settings_font_size_stepper(decrease_auto, auto_limit_label, increase_auto, theme);

        let metered_entity = entity.clone();
        let transfer_policy = components::browser_settings_fieldset(settings.transfers_section_label.clone(), theme)
            .child(components::browser_settings_row(
                settings.background_metered_label.clone(),
                network_description,
                Switch::new("allow-background-on-metered").checked(transfer.allow_background_on_metered()).on_click(move |enabled, _, cx| {
                    metered_entity.update(cx, |page, cx| page.update_transfer(TransferPreferencePatch::AllowBackgroundOnMetered(*enabled), cx));
                }),
                theme,
            ))
            .child(components::browser_settings_row(settings.auto_download_label.clone(), settings.auto_download_description.clone(), auto_choices, theme));

        // "Book display" reads the same shared preference the grids do, so the
        // desktop and mobile settings screens cannot drift apart on it.
        let mut book_display = components::browser_settings_button_group(theme);
        for (index, option) in contract.options.library_views.iter().enumerate() {
            let value = option.value;
            let entity = entity.clone();
            book_display = book_display.child(components::browser_settings_choice_button(format!("library-view-{index}"), option.label.clone(), value == preferences.library_view(), theme).on_click(move |_, _, cx| {
                entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::LibraryView(value), cx));
            }));
        }
        let book_display = components::browser_settings_compact_row(settings.library_view_label.clone(), book_display, theme);

        let mut sections = components::browser_settings_sections()
            .child(appearance_settings)
            .child(catalog_language_policy)
            .child(book_display)
            .child(components::browser_settings_font_row(settings.interface_font_label.clone(), interface_fonts, theme))
            .child(components::browser_settings_font_row(settings.ui_font_size_label.clone(), font_size, theme))
            .child(transfer_policy);

        if server_connected {
            let unavailable_entity = entity.clone();
            sections = sections.child(components::browser_settings_fieldset(settings.library_section_label.clone(), theme).child(components::browser_settings_row(
                settings.show_unavailable_books_label.clone(),
                "Include server books whose local file is currently unavailable.",
                Switch::new("show-unavailable-books").checked(preferences.show_unavailable_books()).on_click(move |enabled, _, cx| {
                    unavailable_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::ShowUnavailableBooks(*enabled), cx));
                }),
                theme,
            )));
        }

        #[cfg(feature = "kobo")]
        {
            let kobo_entity = entity.clone();
            let checked = self.kobo_auto_rotate;
            sections = sections.child(components::browser_settings_fieldset("Kobo", theme).child(components::browser_settings_row(
                "Auto-rotate display",
                "Re-layout UI when Kobo orientation changes. Turn off to keep a fixed screen geometry.",
                Switch::new("kobo-auto-rotate").checked(checked).on_click(move |enabled, _, cx| {
                    kobo_entity.update(cx, |page, cx| page.update_kobo_auto_rotate(*enabled, cx));
                }),
                theme,
            )));
        }

        if let Some(error) = self.error.clone() {
            sections = sections.child(Alert::error("settings-error", error));
        }

        components::browser_settings_page().child(components::browser_settings_content().child(GroupBox::new().fill().child(sections)))
    }
}
