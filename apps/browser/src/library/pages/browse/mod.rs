//! Adapter-driven browsing for folders and subjects.

mod adapter;
/// The shelf pages reuse this module's search field, so the browse page and the
/// author index search alike rather than growing two of them.
pub(super) mod controls;
mod detail;
mod folder_actions;
mod folder_picker;
mod layout;
mod library;
mod page;
mod split;

pub(crate) use adapter::BrowseCrumb;
pub(crate) use folder_picker::{FolderPicker, FolderPickerEvent};
pub(crate) use library::{folder, subject};
pub(crate) use page::BrowsePage;
pub(crate) use split::{BrowsePlaneKind, BrowseSplitPlane};
