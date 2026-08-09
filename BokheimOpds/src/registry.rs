use serde::{Deserialize, Serialize};

pub const CATALOG_REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CatalogAccess {
    Open,
    Membership,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogAuthentication {
    None,
    Basic { username_label: String, password_required: bool },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthentication", tag = "kind", rename_all = "snake_case")]
enum CatalogAuthenticationHuman {
    None,
    Basic { username_label: String, password_required: bool },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthentication")]
enum CatalogAuthenticationBinary {
    None,
    Basic { username_label: String, password_required: bool },
}

impl Serialize for CatalogAuthentication {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            CatalogAuthenticationHuman::serialize(self, serializer)
        } else {
            CatalogAuthenticationBinary::serialize(self, serializer)
        }
    }
}

impl<'de> Deserialize<'de> for CatalogAuthentication {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            CatalogAuthenticationHuman::deserialize(deserializer)
        } else {
            CatalogAuthenticationBinary::deserialize(deserializer)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogSource {
    pub id: String,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    pub url: String,
    pub access: CatalogAccess,
    pub authentication: CatalogAuthentication,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogRegistry {
    pub schema_version: u32,
    pub revision: u64,
    pub catalogs: Vec<CatalogSource>,
}
