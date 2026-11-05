//! UI-owned lookup connections shared across reader popups and windows.
use gpui::{App, Global};
use reader_lookups::Client;

#[derive(Default)]
struct Lookups(Client);

impl Global for Lookups {}

pub(super) fn client(cx: &mut App) -> Client {
    cx.default_global::<Lookups>().0.clone()
}
