//! Presentation records shared across pages.

pub use epub_svelte_lib::library::LibraryFolderEntry as FolderCrumb;
pub use epub_svelte_lib::library::LibrarySubjectEntry as SubjectCrumb;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AccountFlow {
    #[default]
    SignIn,
    Register,
    VerifyEmail,
    RequestPasswordReset,
    ResetPassword,
}

/// Where browsing was left off inside one library.
///
/// Held per library rather than once globally: switching library and switching
/// back is the primary flow of a multi-library app, and a single slot means the
/// library you return to has forgotten where you were.
pub type LibraryPlace = epub_svelte_lib::LibraryPlace;

/// Where the browser reopens after a restart.
///
/// Pages persist only the nested state they own. Folder and subject paths are
/// retained per library; the flat application route is not persisted.
///
/// Sessions written before this became per-library simply lose their browse
/// position — the old flat fields are unknown to serde now and are ignored,
/// which leaves the rest of the session (reader preferences, window geometry)
/// intact. That is why `SESSION_VERSION` is deliberately not bumped for this.
pub type BrowserSession = epub_svelte_lib::BrowserSession;
