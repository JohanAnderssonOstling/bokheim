use crate::preferences::{BrowsingPreferencesContract, ReaderPreferencesContract};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppFeature {
    Home,
    Library,
    Authors,
    Search,
    Store,
}

impl AppFeature {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Library => "Library",
            Self::Authors => "Authors",
            Self::Search => "Search",
            Self::Store => "Store",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct FeatureDefinition {
    pub id: AppFeature,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct FrontendContract {
    pub navigation: Vec<FeatureDefinition>,
    pub browsing_preferences: BrowsingPreferencesContract,
    pub reader_preferences: ReaderPreferencesContract,
}

impl Default for FrontendContract {
    fn default() -> Self {
        let navigation = [AppFeature::Home, AppFeature::Library, AppFeature::Authors, AppFeature::Search, AppFeature::Store].into_iter().map(|id| FeatureDefinition { id, label: id.label().to_string() }).collect();
        Self { navigation, browsing_preferences: BrowsingPreferencesContract::default(), reader_preferences: ReaderPreferencesContract::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_contract_has_each_shared_feature_once() {
        let contract = FrontendContract::default();
        assert_eq!(contract.navigation.iter().map(|feature| feature.id).collect::<Vec<_>>(), vec![AppFeature::Home, AppFeature::Library, AppFeature::Authors, AppFeature::Search, AppFeature::Store]);
        assert!(contract.navigation.iter().all(|feature| feature.label == feature.id.label()));
    }
}
