//! Shared observable state, injected downward.
//!
//! A store is owned by whichever level outlives its contents: universal stores
//! belong to [`BrowserRoot`](crate::root::BrowserRoot), library-scoped ones to
//! [`LibraryRoot`](crate::library::LibraryRoot). Anything read by two siblings
//! lives here rather than being reached through a shared parent.

pub(crate) mod preferences;

pub(crate) use preferences::{LibraryAvailabilityChanged, Preferences, heading_font};
