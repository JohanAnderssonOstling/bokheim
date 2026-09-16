//! Cached pages shared by every library selection.

use gpui::{AnyView, AppContext, Context, Entity, Window};

use super::{BrowserShell, GlobalRoute, Libraries};
use crate::services::AppServices;
use crate::stores::Preferences;
use crate::universal::settings::SettingsPage;

/// Owns the dependencies its pages are built from, so the shell only has to say
/// which page it wants.
pub(super) struct GlobalPages {
    services: AppServices,
    preferences: Entity<Preferences>,
    libraries: Entity<Libraries>,
    settings: Option<Entity<SettingsPage>>,
}

impl GlobalPages {
    pub(super) fn new(services: AppServices, preferences: Entity<Preferences>, libraries: Entity<Libraries>) -> Self {
        Self { services, preferences, libraries, settings: None }
    }

    /// Returns the page for `route`, building it on first use.
    pub(super) fn page(&mut self, route: GlobalRoute, window: &mut Window, cx: &mut Context<BrowserShell>) -> AnyView {
        match route {
            GlobalRoute::Settings => self.settings_page(window, cx).into(),
        }
    }

    /// Reveals the page for `route`, giving it focus where it takes keyboard input.
    pub(super) fn reveal(&mut self, route: GlobalRoute, window: &mut Window, cx: &mut Context<BrowserShell>) {
        match route {
            GlobalRoute::Settings => {
                // Settings is walked with the keyboard, so revealing it puts
                // focus on its first control rather than leaving the keyboard
                // wherever the previous page left it.
                let page = self.settings_page(window, cx);
                page.update(cx, |page, cx| page.focus_first_control(window, cx));
            }
        }
    }

    fn settings_page(&mut self, window: &mut Window, cx: &mut Context<BrowserShell>) -> Entity<SettingsPage> {
        if let Some(page) = &self.settings {
            return page.clone();
        }
        let page = cx.new(|cx| SettingsPage::new(self.services.clone(), self.preferences.clone(), self.libraries.clone(), window, cx));
        self.settings = Some(page.clone());
        page
    }
}
