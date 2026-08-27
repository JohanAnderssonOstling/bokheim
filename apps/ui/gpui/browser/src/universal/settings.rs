//! Account, appearance, library presentation and desktop update settings.
//!
//! Universal: everything it edits is account-scoped rather than library-scoped,
//! so it edits the shared [`Preferences`] store and every library page observes
//! the result.
//!
//! The account and its sync state are the first section rather than a
//! destination of their own: signing in is something you do once, and the
//! things it governs — quota, which libraries sync — are settings like any
//! other. [`SyncPage`] keeps owning that state and is embedded here.

use app_preferences::{ApplicationAppearance, ApplicationTheme, BrowsingPreferencePatch, UI_FONT_SIZE_MAX, UI_FONT_SIZE_MIN, UiFontFamily};
use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, Render, SharedString, Subscription, Window};
use gpui_component::IconName;
use gpui_component::alert::Alert;
use gpui_component::group_box::{GroupBox, GroupBoxVariants};
#[cfg(feature = "kobo")]
use gpui_component::switch::Switch;
use ui_components as components;

use crate::navigation::NavigationState;
use crate::services::{AppServices, DesktopUpdate};
use crate::stores::Preferences;
use crate::universal::sync::SyncPage;

enum DesktopUpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(DesktopUpdate),
    Installing { version: String },
    ReadyToRestart { version: String },
    Failed(String),
}

pub(crate) struct SettingsPage {
    services: AppServices,
    preferences: Entity<Preferences>,
    account: Entity<SyncPage>,
    error: Option<SharedString>,
    desktop_update: DesktopUpdateState,
    #[cfg(feature = "kobo")]
    kobo_auto_rotate: bool,
    _account_subscription: Subscription,
}

impl SettingsPage {
    pub(crate) fn new(services: AppServices, preferences: Entity<Preferences>, active_library: Entity<NavigationState>, #[cfg(feature = "kobo")] kobo_auto_rotate: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let desktop_update = services.desktop_updater.as_ref().and_then(|updater| updater.available_update()).map(DesktopUpdateState::Available).unwrap_or(DesktopUpdateState::Idle);
        let account = cx.new(|cx| SyncPage::new(services.clone(), active_library, window, cx));
        // The account section refreshes on its own — transfers finishing, a
        // sign-in completing — and this page has to repaint when it does.
        let account_subscription = cx.observe(&account, |_, _, cx| cx.notify());
        Self {
            services,
            preferences,
            account,
            error: None,
            desktop_update,
            #[cfg(feature = "kobo")]
            kobo_auto_rotate,
            _account_subscription: account_subscription,
        }
    }

    fn update_browsing(&mut self, patch: BrowsingPreferencePatch, cx: &mut Context<Self>) {
        let result = self.preferences.update(cx, |preferences, cx| preferences.update_browsing(patch, cx));
        self.error = result.err().map(|error| SharedString::from(format!("Could not save setting: {error}")));
        cx.notify();
    }

    fn check_desktop_update(&mut self, cx: &mut Context<Self>) {
        let Some(updater) = self.services.desktop_updater.clone() else { return };
        self.desktop_update = DesktopUpdateState::Checking;
        cx.notify();
        let result = updater.check();
        cx.spawn(async move |page, cx| {
            let outcome = result.recv().await.unwrap_or_else(|_| Err("The desktop update worker stopped unexpectedly".to_owned()));
            let _ = page.update(cx, |page, cx| {
                page.desktop_update = match outcome {
                    Ok(Some(update)) => {
                        updater.set_available_update(Some(update.clone()));
                        DesktopUpdateState::Available(update)
                    }
                    Ok(None) => {
                        updater.set_available_update(None);
                        DesktopUpdateState::UpToDate
                    }
                    Err(error) => DesktopUpdateState::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn install_desktop_update(&mut self, update: DesktopUpdate, cx: &mut Context<Self>) {
        let Some(updater) = self.services.desktop_updater.clone() else { return };
        let version = update.version.clone();
        self.desktop_update = DesktopUpdateState::Installing { version: version.clone() };
        cx.notify();
        let result = updater.install(update);
        cx.spawn(async move |page, cx| {
            let outcome = result.recv().await.unwrap_or_else(|_| Err("The desktop update worker stopped unexpectedly".to_owned()));
            let _ = page.update(cx, |page, cx| {
                page.desktop_update = match outcome {
                    Ok(()) => DesktopUpdateState::ReadyToRestart { version },
                    Err(error) => DesktopUpdateState::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn restart_after_desktop_update(&mut self, cx: &mut Context<Self>) {
        let Some(updater) = self.services.desktop_updater.clone() else { return };
        match updater.restart() {
            Ok(()) => cx.quit(),
            Err(error) => {
                self.desktop_update = DesktopUpdateState::Failed(error);
                cx.notify();
            }
        }
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
        let preferences = self.preferences.read(cx).browsing();
        let entity = cx.entity();

        let current_font_size = preferences.ui_font_size_px();
        let [minimum_font_size, maximum_font_size] = [UI_FONT_SIZE_MIN, UI_FONT_SIZE_MAX];

        // Light/dark is a separate axis from which theme, but both are compact
        // choices and belong together rather than in tall descriptive cards.
        let mut appearances = components::browser_settings_button_group(theme);
        for (index, value) in [ApplicationAppearance::System, ApplicationAppearance::Light, ApplicationAppearance::Dark].into_iter().enumerate() {
            let entity = entity.clone();
            appearances = appearances.child(components::browser_settings_choice_button(format!("application-appearance-{index}"), value.label(), value == preferences.appearance(), theme).on_click(move |_, _, cx| {
                entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::Appearance(value), cx));
            }));
        }

        let mut application_themes = components::browser_settings_button_group(theme);
        for (index, value) in [ApplicationTheme::GrecoFuturistic, ApplicationTheme::LibrarySerif, ApplicationTheme::ClassicWhite].into_iter().enumerate() {
            let entity = entity.clone();
            application_themes = application_themes.child(components::browser_settings_choice_button(format!("application-theme-{index}"), value.label(), value == preferences.theme(), theme).on_click(move |_, _, cx| {
                entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::Theme(value), cx));
            }));
        }
        let appearance_settings = components::browser_settings_fieldset("Appearance", theme).child(components::browser_settings_compact_row("Light or dark", appearances, theme)).child(components::browser_settings_compact_row(
            "Application theme",
            application_themes,
            theme,
        ));

        let mut interface_fonts = components::browser_settings_button_group(theme);
        for (index, value) in [UiFontFamily::SansSerif, UiFontFamily::Serif].into_iter().enumerate() {
            let interface_entity = entity.clone();
            interface_fonts =
                interface_fonts.child(components::browser_settings_choice_button(format!("interface-font-{index}"), value.label(), value == preferences.interface_font(), theme).font_family(value.font_name()).on_click(move |_, _, cx| {
                    interface_entity.update(cx, |page, cx| page.update_browsing(BrowsingPreferencePatch::InterfaceFont(value), cx));
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

        let mut sections = components::browser_settings_sections().child(appearance_settings).child(components::browser_settings_compact_row("Interface font", interface_fonts, theme)).child(components::browser_settings_compact_row(
            "UI font size",
            font_size,
            theme,
        ));

        if let Some(updater) = self.services.desktop_updater.as_ref() {
            let current_version = updater.current_version().to_owned();
            let (description, control) = match &self.desktop_update {
                DesktopUpdateState::Idle => {
                    let action = entity.clone();
                    (
                        format!("Version {current_version}. Updates are available for directly downloaded Bokheim builds."),
                        components::secondary_action_button("check-desktop-update", "Check for updates", IconName::Redo2, false).on_click(move |_, _, cx| action.update(cx, |page, cx| page.check_desktop_update(cx))),
                    )
                }
                DesktopUpdateState::Checking => (format!("Checking GitHub Releases from version {current_version}…"), components::secondary_action_button("checking-desktop-update", "Checking…", IconName::Redo2, true)),
                DesktopUpdateState::UpToDate => {
                    let action = entity.clone();
                    (
                        format!("Version {current_version} is current."),
                        components::secondary_action_button("recheck-desktop-update", "Check again", IconName::Redo2, false).on_click(move |_, _, cx| action.update(cx, |page, cx| page.check_desktop_update(cx))),
                    )
                }
                DesktopUpdateState::Available(update) => {
                    let available = update.clone();
                    let action = entity.clone();
                    (
                        format!("{} is available. The download is verified before the current application is replaced.", update.release_name),
                        components::secondary_action_button("install-desktop-update", format!("Install {}", update.version), IconName::ArrowRight, false)
                            .on_click(move |_, _, cx| action.update(cx, |page, cx| page.install_desktop_update(available.clone(), cx))),
                    )
                }
                DesktopUpdateState::Installing { version } => (format!("Downloading and verifying Bokheim {version}…"), components::secondary_action_button("installing-desktop-update", "Installing…", IconName::ArrowRight, true)),
                DesktopUpdateState::ReadyToRestart { version } => {
                    let action = entity.clone();
                    (
                        format!("Bokheim {version} is ready. Restart Bokheim to finish installing the update."),
                        components::secondary_action_button("restart-desktop-update", "Restart Bokheim", IconName::ArrowRight, false).on_click(move |_, _, cx| action.update(cx, |page, cx| page.restart_after_desktop_update(cx))),
                    )
                }
                DesktopUpdateState::Failed(error) => {
                    let action = entity.clone();
                    (format!("Update failed: {error}"), components::secondary_action_button("retry-desktop-update", "Try again", IconName::Redo2, false).on_click(move |_, _, cx| action.update(cx, |page, cx| page.check_desktop_update(cx))))
                }
            };
            sections = sections.child(components::browser_settings_fieldset("Updates", theme).child(components::browser_settings_row("Bokheim updates", description, control, theme)));
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

        // The account leads the page: it is the one section that can be in a
        // state the user has to act on, and the rest are preferences that are
        // always simply on or off.
        let account = self.account.update(cx, |page, cx| page.settings_section(cx));
        components::browser_settings_page().child(components::browser_settings_content().child(account).child(GroupBox::new().fill().child(sections)))
    }
}
