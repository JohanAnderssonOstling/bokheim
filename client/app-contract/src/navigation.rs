//! Transport-neutral browser navigation and persisted-place records.

use std::collections::HashMap;
use std::hash::Hash;

/// Stable persisted location within a hierarchical browser page.
///
/// The previous session shape stored full hierarchy rows, including changing
/// inventory counts. `name` is accepted when reading those sessions so the
/// navigation itself survives this narrower representation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NavigationCrumb<Id> {
    pub id: Id,
    #[serde(alias = "name")]
    pub label: String,
}

/// The hierarchy paths visited in one library.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "FolderCrumb: serde::Serialize, SubjectCrumb: serde::Serialize", deserialize = "FolderCrumb: serde::Deserialize<'de>, SubjectCrumb: serde::Deserialize<'de>"))]
pub struct LibraryPlace<FolderCrumb, SubjectCrumb> {
    #[serde(default)]
    pub folder_path: Vec<FolderCrumb>,
    #[serde(default)]
    pub subject_path: Vec<SubjectCrumb>,
}

impl<FolderCrumb, SubjectCrumb> Default for LibraryPlace<FolderCrumb, SubjectCrumb> {
    fn default() -> Self {
        Self { folder_path: Vec::new(), subject_path: Vec::new() }
    }
}

/// Browser state shared by native hosts and mirrored by generated web types.
/// Each page persists only its own nested place within a library.
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

#[cfg(test)]
mod tests {
    use super::NavigationCrumb;

    #[test]
    fn persisted_navigation_crumb_reads_the_previous_hierarchy_row_shape() {
        let crumb: NavigationCrumb<String> = serde_json::from_value(serde_json::json!({
            "id": "child",
            "name": "Child",
            "book_count": 42,
            "downloaded_book_count": 7
        }))
        .unwrap();

        assert_eq!(crumb, NavigationCrumb { id: "child".to_owned(), label: "Child".to_owned() });
        assert_eq!(serde_json::to_value(crumb).unwrap(), serde_json::json!({ "id": "child", "label": "Child" }));
    }
}
