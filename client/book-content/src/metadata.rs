//! Declared bibliographic fields shared by every container parser.

mod definitions;
pub use definitions::{CreatorRole, IdentifierScheme, parse_creator_role, parse_identifier_scheme};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataIdentifier {
    pub value: String,
    pub scheme: IdentifierScheme,
    /// Raw identifier `identifier-type` refinement. Keeping this evidence avoids
    /// losing proprietary or future identifier systems that Bokheim does not
    /// yet understand.
    pub identifier_types: Vec<MetadataIdentifierType>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataIdentifierType {
    pub value: String,
    pub scheme: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MetadataCredit {
    pub id: Option<String>,
    pub name: String,            // Display name (from text content)
    pub file_as: Option<String>, // Sort name (from opf:file-as)
    pub role: CreatorRole,
}

impl MetadataCredit {
    pub fn author(name: String) -> Self {
        Self { id: None, name, file_as: None, role: CreatorRole::author() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeclaredMetadata {
    /// Container metadata location, when the format has a named document.
    pub source_path: Option<String>,
    pub title: String,
    pub alternate_titles: Vec<MetadataTitle>,
    pub identifiers: Vec<MetadataIdentifier>,
    pub creators: Vec<MetadataCredit>,
    pub contributors: Vec<MetadataCredit>,
    pub description: String,
    pub publishers: Vec<String>,
    pub languages: Vec<String>,
    pub dates: Vec<MetadataDate>,
    pub subjects: Vec<MetadataSubject>,
    pub rights: Vec<String>,
    pub sources: Vec<String>,
    pub types: Vec<String>,
    pub formats: Vec<String>,
    pub relations: Vec<String>,
    pub coverage: Vec<String>,
    pub properties: Vec<MetadataProperty>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataProperty {
    pub id: Option<String>,
    pub property: String,
    pub value: String,
    pub refines: Option<String>,
    pub scheme: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataDate {
    pub source: String,
    pub id: Option<String>,
    pub value: String,
    pub event: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataSubject {
    pub id: Option<String>,
    pub value: String,
    /// Exact metadata element or property that declared this value.
    pub source: String,
    pub authority: Option<String>,
    pub code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataTitle {
    pub id: Option<String>,
    pub value: String,
    pub title_type: Option<String>,
    pub sort_as: Option<String>,
}
