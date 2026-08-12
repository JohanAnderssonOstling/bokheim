use serde::{Deserialize, Serialize};

pub const CATALOG_REGISTRY_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogSource {
    pub id: String,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogRegistry {
    pub schema_version: u32,
    pub revision: u64,
    pub catalogs: Vec<CatalogSource>,
}
