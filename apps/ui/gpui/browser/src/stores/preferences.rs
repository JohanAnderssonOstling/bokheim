//! Universal preference state.
//!
//! `backend` stores browsing and transfer preferences per account, not per
//! library — neither getter takes a library id, and browsing changes enqueue to
//! the signed-in account. So this store lives at the root and is injected
//! downward. Settings edits it, appearance refreshes all windows, and the
//! selected-library root observes only availability-policy changes.

use epub_svelte_lib::app::preferences::{BrowsingPreferencePatch, BrowsingPreferences, TransferPreferencePatch, TransferPreferences};
use gpui::{px, App, Context, EventEmitter, SharedString};
use gpui_component::{Theme, ThemeTokens};

use crate::services::AppServices;

/// Whether the platform reports a dark colour scheme.
///
/// On Linux this comes from the XDG desktop portal's `color-scheme`; the Kobo
/// platform always reports light, which is the right answer for e-ink and means
/// the explicit Light/Dark choice is what matters there.
pub(crate) fn system_prefers_dark(cx: &App) -> bool {
    matches!(cx.window_appearance(), gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark)
}

/// The backend's availability policy changed, so ordinary browse queries are
/// stale. Appearance changes use GPUI's window refresh instead.
pub(crate) struct LibraryAvailabilityChanged;

/// The book display preference changed, so retained grids need to switch
/// presentation. Separate from [`LibraryAvailabilityChanged`] because nothing
/// needs re-querying: the same cards are drawn a different way.
pub(crate) struct LibraryDisplayChanged(pub(crate) app_contract::preferences::LibraryView);

pub(crate) fn interface_font(cx: &App) -> SharedString {
    Theme::global(cx).font_family.clone()
}

pub(crate) struct Preferences {
    services: AppServices,
    browsing: BrowsingPreferences,
    transfer: TransferPreferences,
}

impl EventEmitter<LibraryAvailabilityChanged> for Preferences {}
impl EventEmitter<LibraryDisplayChanged> for Preferences {}

impl Preferences {
    pub(crate) fn new(services: AppServices, cx: &mut App) -> Self {
        let browsing = services.backend.browsing_preferences().unwrap_or_else(|error| {
            log::error!("failed to load browsing preferences: {error}");
            BrowsingPreferences::default()
        });
        let transfer = services.backend.transfer_preferences().unwrap_or_else(|error| {
            log::error!("failed to load transfer preferences: {error}");
            TransferPreferences::default()
        });
        apply_browsing_appearance(browsing.clone(), cx);
        Self { services, browsing, transfer }
    }

    pub(crate) fn browsing(&self) -> BrowsingPreferences {
        self.browsing.clone()
    }

    pub(crate) const fn transfer(&self) -> TransferPreferences {
        self.transfer
    }

    pub(crate) fn interface_font_name(&self) -> SharedString {
        SharedString::from(self.browsing.interface_font().font_name())
    }

    pub(crate) fn ui_font_size_px(&self) -> u8 {
        self.browsing.ui_font_size_px()
    }

    /// Applies a browsing patch. Returns the error text on failure so the
    /// Settings page can surface it without this store knowing about pages.
    pub(crate) fn update_browsing(&mut self, patch: BrowsingPreferencePatch, cx: &mut Context<Self>) -> Result<(), String> {
        match self.services.backend.update_browsing_preference(patch) {
            Ok(preferences) => {
                let previous = self.browsing.clone();
                self.browsing = preferences;
                let appearance_changed = previous.theme() != self.browsing.theme()
                    || previous.appearance() != self.browsing.appearance()
                    || previous.interface_font() != self.browsing.interface_font()
                    || previous.ui_font_size_px() != self.browsing.ui_font_size_px();
                if appearance_changed {
                    apply_browsing_appearance(self.browsing.clone(), cx);
                }
                if previous.show_unavailable_books() != self.browsing.show_unavailable_books() {
                    cx.emit(LibraryAvailabilityChanged);
                }
                if previous.library_view() != self.browsing.library_view() {
                    cx.emit(LibraryDisplayChanged(self.browsing.library_view()));
                }
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub(crate) fn set_transfer(&mut self, preferences: TransferPreferences) {
        self.transfer = preferences;
    }

    pub(crate) fn transfer_patch(&self, patch: TransferPreferencePatch) -> impl std::future::Future<Output = Result<TransferPreferences, String>> + 'static {
        let core = self.services.backend.clone();
        async move { core.update_transfer_preference(patch).await.map_err(|error| error.to_string()) }
    }
}

/// Pushes the browsing palette and fonts into the global GPUI theme.
///
/// Free-standing because it mutates application-global state rather than the
/// store, and is called once at construction before any entity exists.
pub(crate) fn apply_browsing_appearance(preferences: BrowsingPreferences, cx: &mut App) {
    let resolved = preferences.appearance().resolve(system_prefers_dark(cx));
    let palette = preferences.theme().palette(resolved);
    let surface: gpui::Hsla = gpui::rgb(palette.surface_bg).into();
    let raised: gpui::Hsla = gpui::rgb(palette.raised_bg).into();
    let text: gpui::Hsla = gpui::rgb(palette.ink_text).into();
    let muted_text: gpui::Hsla = gpui::rgb(palette.subtle_text).into();
    let accent: gpui::Hsla = gpui::rgb(palette.accent).into();
    let border: gpui::Hsla = gpui::rgb(palette.border).into();
    // Composited over whatever it sits on, so one value reads correctly on the
    // page, a panel and a raised card alike.
    let hover: gpui::Hsla = gpui::rgba(palette.hover_overlay).into();
    let active: gpui::Hsla = gpui::rgb(palette.active_highlight).into();
    let danger: gpui::Hsla = gpui::rgb(palette.danger_text).into();
    let danger_bg: gpui::Hsla = gpui::rgb(palette.danger_bg).into();
    let warning: gpui::Hsla = gpui::rgb(palette.warning_text).into();
    let warning_bg: gpui::Hsla = gpui::rgb(palette.warning_bg).into();

    let application_theme = Theme::global_mut(cx);
    #[cfg(feature = "kobo")]
    {
        application_theme.font_family = "Terminus (TTF)".into();
    }
    #[cfg(not(feature = "kobo"))]
    {
        application_theme.font_family = preferences.interface_font().font_name().into();
    }
    application_theme.font_size = px(preferences.ui_font_size_px() as f32);
    application_theme.radius = px(0.0);
    application_theme.radius_lg = px(0.0);
    let colors = &mut application_theme.colors;
    // Use the lighter surface tone as the native application's continuous
    // canvas. Raised surfaces still retain their separate card/control color.
    colors.background = surface;
    colors.foreground = text;
    colors.border = border;
    colors.accent = active;
    colors.accent_foreground = text;
    colors.group_box = raised;
    colors.group_box_foreground = text;
    colors.input = border;
    colors.muted = hover;
    colors.muted_foreground = muted_text;
    colors.popover = raised;
    colors.popover_foreground = text;
    colors.primary = accent;
    colors.primary_active = accent;
    colors.primary_hover = accent;
    colors.primary_foreground = raised;
    colors.secondary = surface;
    colors.secondary_active = active;
    colors.secondary_hover = hover;
    colors.secondary_foreground = text;
    colors.button = surface;
    colors.button_active = active;
    colors.button_hover = hover;
    colors.button_foreground = text;
    colors.button_primary = accent;
    colors.button_primary_active = accent;
    colors.button_primary_hover = accent;
    colors.button_primary_foreground = raised;
    colors.button_secondary = surface;
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
    colors.sidebar = surface;
    colors.sidebar_accent = active;
    colors.sidebar_accent_foreground = accent;
    colors.sidebar_border = border;
    colors.sidebar_foreground = text;
    colors.sidebar_primary = accent;
    colors.sidebar_primary_foreground = raised;
    colors.title_bar = surface;
    colors.title_bar_border = border;
    colors.danger = danger_bg;
    colors.danger_foreground = danger;
    colors.warning = warning_bg;
    colors.warning_foreground = warning;
    colors.ring = accent;
    application_theme.tokens = ThemeTokens::from(application_theme.colors);
    cx.refresh_windows();
}
