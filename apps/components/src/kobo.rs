//! The selected hardware profile also controls which device controls are shown.
use gpui::{App, Global};
use kobo_profile::{Frontlight, Profile};

pub struct DeviceProfile(pub &'static Profile);
impl Global for DeviceProfile {}

pub fn has_gyroscope(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| profile.0.has_gyroscope())
}

pub fn has_frontlight(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| profile.0.frontlight() != Frontlight::None)
}

pub fn has_warmth(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| !matches!(profile.0.frontlight(), Frontlight::None | Frontlight::Standard))
}

pub fn has_bluetooth(cx: &App) -> bool {
    cx.try_global::<DeviceProfile>().is_some_and(|profile| profile.0.has_bluetooth())
}
