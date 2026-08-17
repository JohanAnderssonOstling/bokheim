//! Transport-neutral browser navigation and persisted-place records.

use std::collections::HashMap;
use std::hash::Hash;

use crate::frontend::LibraryFeature;

/// The last library-owned page and hierarchy paths visited in one library.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "FolderCrumb: serde::Serialize, SubjectCrumb: serde::Serialize", deserialize = "FolderCrumb: serde::Deserialize<'de>, SubjectCrumb: serde::Deserialize<'de>"))]
pub struct LibraryPlace<FolderCrumb, SubjectCrumb> {
    #[serde(default)]
    pub route: LibraryFeature,
    #[serde(default)]
    pub folder_path: Vec<FolderCrumb>,
    #[serde(default)]
    pub subject_path: Vec<SubjectCrumb>,
}

impl<FolderCrumb, SubjectCrumb> Default for LibraryPlace<FolderCrumb, SubjectCrumb> {
    fn default() -> Self {
        Self { route: LibraryFeature::default(), folder_path: Vec::new(), subject_path: Vec::new() }
    }
}

/// Browser state shared by native hosts and mirrored by generated web types.
/// Universal pages are deliberately absent: they never replace the library
/// place underneath them.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "LibraryId: serde::Serialize + Eq + Hash, FolderCrumb: serde::Serialize, SubjectCrumb: serde::Serialize",
    deserialize = "LibraryId: serde::Deserialize<'de> + Eq + Hash, FolderCrumb: serde::Deserialize<'de>, SubjectCrumb: serde::Deserialize<'de>"
))]
pub struct BrowserSession<LibraryId: Eq + Hash, FolderCrumb, SubjectCrumb> {
    pub active_library_id: Option<LibraryId>,
    #[serde(default)]
    pub places: HashMap<LibraryId, LibraryPlace<FolderCrumb, SubjectCrumb>>,
}

impl<LibraryId: Eq + Hash, FolderCrumb, SubjectCrumb> Default for BrowserSession<LibraryId, FolderCrumb, SubjectCrumb> {
    fn default() -> Self {
        Self { active_library_id: None, places: HashMap::new() }
    }
}

impl<LibraryId, FolderCrumb, SubjectCrumb> BrowserSession<LibraryId, FolderCrumb, SubjectCrumb>
where
    LibraryId: Clone + Eq + Hash,
    FolderCrumb: Clone,
    SubjectCrumb: Clone,
{
    pub fn place(&self, library_id: &LibraryId) -> LibraryPlace<FolderCrumb, SubjectCrumb> {
        self.places.get(library_id).cloned().unwrap_or_default()
    }

    pub fn retain_places(&mut self, present: impl Fn(&LibraryId) -> bool) {
        self.places.retain(|library_id, _| present(library_id));
    }
}
