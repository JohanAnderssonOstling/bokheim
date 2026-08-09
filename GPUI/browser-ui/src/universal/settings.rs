//! Appearance, library presentation and transfer policy.
//!
//! Universal: everything it edits is account-scoped rather than library-scoped,
//! so it edits the shared [`Preferences`] store and every library page observes
//! the result.

use bokheim_ui_components as components;
use epub_svelte_lib::app::preferences::{BrowsingPreferencePatch, TransferPreferencePatch};
use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Render, SharedString, Window};
use gpui_component::switch::Switch;

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
        let heading_font = self.preferences.read(cx).heading_font_name();
        let entity = cx.entity();

        let current_font_size = preferences.ui_font_size_px();
        let [minimum_font_size, maximum_font_size] = contract.constraints.ui_font_size_px;

        let mut application_themes = components::browser_settings_fieldset(settings.appearance_section_label.clone(), theme);
        for (index, option) in contract.options.themes.iter().enumerate() {
            let value = option.value;
            let entity = entity.clone();
            application_themes = application_themes.child(
                components::browser_settings_choice_card(format!("application-theme-{index}"), option.label.clone(), option.description.clone(), value == preferences.theme(), heading_font.clone(), theme).on_click(move |checked, _, cx| {
                    if *checked {
                        entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::Theme(value), cx));
                    }
                }),
            );
        }

        let mut interface_fonts = components::browser_settings_font_choices(theme);
        let mut heading_fonts = components::browser_settings_font_choices(theme);
        for (index, option) in contract.options.fonts.iter().enumerate() {
            let value = option.value;
            let interface_entity = entity.clone();
            interface_fonts = interface_fonts.child(components::browser_settings_font_button(format!("interface-font-{index}"), option.font_name.clone(), value == preferences.interface_font(), option.font_name.clone(), theme).on_click(
                move |_, _, cx| {
                    interface_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::InterfaceFont(value), cx));
                },
            ));
            let heading_entity = entity.clone();
            heading_fonts =
                heading_fonts.child(components::browser_settings_font_button(format!("heading-font-{index}"), option.font_name.clone(), value == preferences.heading_font(), option.font_name.clone(), theme).on_click(move |_, _, cx| {
                    heading_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::HeadingFont(value), cx));
                }));
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

        let mut sections = components::browser_settings_sections()
            .child(application_themes)
            .child(components::browser_settings_font_row(settings.interface_font_label.clone(), interface_fonts, theme))
            .child(components::browser_settings_font_row(settings.heading_font_label.clone(), heading_fonts, theme))
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
            sections = sections.child(components::browser_inline_message(error, theme));
        }

        components::browser_settings_page().child(components::browser_settings_content().child(components::browser_settings_card(theme).child(sections)))
    }
}
