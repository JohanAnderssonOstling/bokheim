//! Work-level popularity evidence from an immutable Open Library ratings dump.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct WorkPopularity {
    pub open_library_work_id: String,
    /// Number of ratings, measuring participation rather than rating quality.
    pub rating_count: u64,
    /// Date of the bibliographic snapshot associated with this answer.
    /// The ratings input is independently supplied to the same immutable build.
    pub snapshot_date: String,
}

/// Preserve a single work's evidence without counting it again for each ISBN.
/// Unknown work identities and conflicting observations cannot establish popularity.
pub fn unique_work_popularity<'a>(matches: impl IntoIterator<Item = &'a crate::RichWorkMetadataMatch>) -> Option<WorkPopularity> {
    let mut work = None;
    let mut popularity = None;
    for matched in matches {
        let Some(id) = matched.open_library_work_id.as_deref() else { continue };
        if work.is_some_and(|previous| previous != id) {
            return None;
        }
        work = Some(id);
        if let Some(observation) = &matched.popularity {
            if observation.open_library_work_id != id || popularity.as_ref().is_some_and(|previous| previous != observation) {
                return None;
            }
            popularity = Some(observation.clone());
        }
    }
    popularity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{merge_rich_work_matches, RichWorkMetadataMatch};

    fn matched(work: &str, count: Option<u64>) -> RichWorkMetadataMatch {
        RichWorkMetadataMatch {
            open_library_work_id: Some(work.into()),
            exact_edition_ids: vec![],
            classifications: vec![],
            authors: vec![],
            description: None,
            subjects: vec![],
            popularity: count.map(|rating_count| WorkPopularity { open_library_work_id: work.into(), rating_count, snapshot_date: "2026-09-01".into() }),
        }
    }

    #[test]
    fn repeated_isbns_preserve_one_work_count() {
        let first = matched("OL10W", Some(12));
        let second = matched("OL10W", Some(12));
        let missing = matched("OL10W", None);
        let merged = merge_rich_work_matches([&first, &second, &missing], |_, _| vec![]);
        assert_eq!(merged.popularity, first.popularity);
    }

    #[test]
    fn ambiguous_work_identity_and_conflicting_observations_are_unknown() {
        let first = matched("OL10W", Some(12));
        let other = matched("OL20W", None);
        let conflict = matched("OL10W", Some(13));
        assert_eq!(unique_work_popularity([&first, &other]), None);
        assert_eq!(unique_work_popularity([&first, &conflict]), None);
        assert_eq!(unique_work_popularity([&matched("OL10W", None)]), None);
        let mut wrong_owner = first.clone();
        wrong_owner.popularity.as_mut().unwrap().open_library_work_id = "OL20W".into();
        assert_eq!(unique_work_popularity([&wrong_owner]), None);
    }

    #[test]
    fn old_json_without_popularity_remains_readable() {
        let mut json = serde_json::to_value(matched("OL10W", None)).unwrap();
        json.as_object_mut().unwrap().remove("popularity");
        assert!(serde_json::from_value::<RichWorkMetadataMatch>(json).unwrap().popularity.is_none());
    }
}
