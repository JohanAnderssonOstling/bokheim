use serde::{Deserialize, Serialize};
use specta::Type;

macro_rules! dual_serde {
    ($type:ty, $human:ident, $binary:ident) => {
        impl Serialize for $type {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if serializer.is_human_readable() {
                    $human::serialize(self, serializer)
                } else {
                    $binary::serialize(self, serializer)
                }
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                if deserializer.is_human_readable() {
                    $human::deserialize(deserializer)
                } else {
                    $binary::deserialize(deserializer)
                }
            }
        }
    };
}

const MAX_CATALOG_ID_BYTES: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(transparent)]
pub struct CatalogSourceId(String);

impl CatalogSourceId {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() || value.len() > MAX_CATALOG_ID_BYTES || !value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return Err("catalog source id must contain only lowercase ASCII letters, digits, and hyphens".to_owned());
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CatalogSourceId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogProviderKind {
    Opds,
    Overdrive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogCapability {
    Browse,
    Search,
    Facets,
    Import,
    ExternalFulfillment,
    Borrow,
    Holds,
    Loans,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogPasswordRequirement {
    Required,
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq, Type)]
#[specta(type = CatalogAuthenticationFrontend)]
pub enum CatalogAuthentication {
    None,
    Basic { username_label: String, password: CatalogPasswordRequirement },
    External { action_label: String, url: String },
}

#[derive(Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[specta(inline, collect = false)]
enum CatalogAuthenticationFrontend {
    None,
    Basic { username_label: String, password: CatalogPasswordRequirement },
    External { action_label: String, url: String },
}

impl From<&CatalogAuthentication> for CatalogAuthenticationFrontend {
    fn from(value: &CatalogAuthentication) -> Self {
        match value {
            CatalogAuthentication::None => Self::None,
            CatalogAuthentication::Basic { username_label, password } => Self::Basic { username_label: username_label.clone(), password: *password },
            CatalogAuthentication::External { action_label, url } => Self::External { action_label: action_label.clone(), url: url.clone() },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthentication", tag = "kind", rename_all = "snake_case")]
enum CatalogAuthenticationHuman {
    None,
    Basic { username_label: String, password: CatalogPasswordRequirement },
    External { action_label: String, url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthentication")]
enum CatalogAuthenticationBinary {
    None,
    Basic { username_label: String, password: CatalogPasswordRequirement },
    External { action_label: String, url: String },
}

dual_serde!(CatalogAuthentication, CatalogAuthenticationHuman, CatalogAuthenticationBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogSource {
    pub id: CatalogSourceId,
    pub provider: CatalogProviderKind,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    pub authentication: CatalogAuthentication,
    pub capabilities: Vec<CatalogCapability>,
}

#[derive(Clone, Debug, PartialEq, Eq, Type)]
#[specta(type = CatalogLinkTargetFrontend)]
pub enum CatalogLinkTarget {
    Page { locator: String },
    External { url: String },
}

#[derive(Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[specta(inline, collect = false)]
enum CatalogLinkTargetFrontend {
    Page { locator: String },
    External { url: String },
}

impl From<&CatalogLinkTarget> for CatalogLinkTargetFrontend {
    fn from(value: &CatalogLinkTarget) -> Self {
        match value {
            CatalogLinkTarget::Page { locator } => Self::Page { locator: locator.clone() },
            CatalogLinkTarget::External { url } => Self::External { url: url.clone() },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogLinkTarget", tag = "kind", rename_all = "snake_case")]
enum CatalogLinkTargetHuman {
    Page { locator: String },
    External { url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogLinkTarget")]
enum CatalogLinkTargetBinary {
    Page { locator: String },
    External { url: String },
}

dual_serde!(CatalogLinkTarget, CatalogLinkTargetHuman, CatalogLinkTargetBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogLink {
    pub title: String,
    pub target: CatalogLinkTarget,
    pub number_of_items: Option<u32>,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSearchField {
    Query,
    Title,
    Author,
}

#[derive(Clone, Debug, PartialEq, Eq, Type)]
#[specta(type = CatalogSearchDefinitionFrontend)]
pub enum CatalogSearchDefinition {
    OpdsUriTemplate { base_url: String, template: String },
    OpdsOpenSearchDescription { url: String },
    Overdrive,
}

#[derive(Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[specta(inline, collect = false)]
enum CatalogSearchDefinitionFrontend {
    OpdsUriTemplate { base_url: String, template: String },
    OpdsOpenSearchDescription { url: String },
    Overdrive,
}

impl From<&CatalogSearchDefinition> for CatalogSearchDefinitionFrontend {
    fn from(value: &CatalogSearchDefinition) -> Self {
        match value {
            CatalogSearchDefinition::OpdsUriTemplate { base_url, template } => Self::OpdsUriTemplate { base_url: base_url.clone(), template: template.clone() },
            CatalogSearchDefinition::OpdsOpenSearchDescription { url } => Self::OpdsOpenSearchDescription { url: url.clone() },
            CatalogSearchDefinition::Overdrive => Self::Overdrive,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogSearchDefinition", tag = "kind", rename_all = "snake_case")]
enum CatalogSearchDefinitionHuman {
    OpdsUriTemplate { base_url: String, template: String },
    OpdsOpenSearchDescription { url: String },
    Overdrive,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogSearchDefinition")]
enum CatalogSearchDefinitionBinary {
    OpdsUriTemplate { base_url: String, template: String },
    OpdsOpenSearchDescription { url: String },
    Overdrive,
}

dual_serde!(CatalogSearchDefinition, CatalogSearchDefinitionHuman, CatalogSearchDefinitionBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogSearch {
    pub definition: CatalogSearchDefinition,
    pub fields: Vec<CatalogSearchField>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogSearchQuery {
    pub query: String,
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogWideSearchResult {
    pub query: String,
    pub groups: Vec<CatalogWideSearchGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogWideSearchGroup {
    pub source: CatalogSource,
    pub outcome: CatalogWideSearchOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogPublicationIdentifierScheme {
    Isbn,
    Asin,
    Doi,
    Oclc,
    Lccn,
    Google,
    Goodreads,
    Kobo,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct CatalogPublicationIdentifier {
    pub scheme: CatalogPublicationIdentifierScheme,
    pub value: String,
}

impl CatalogPublicationIdentifier {
    pub fn normalized_value(&self) -> Option<String> {
        use sync_common::PublicationIdentifierScheme as SyncScheme;

        let scheme = match self.scheme {
            CatalogPublicationIdentifierScheme::Isbn => SyncScheme::Isbn,
            CatalogPublicationIdentifierScheme::Asin => SyncScheme::Asin,
            CatalogPublicationIdentifierScheme::Doi => SyncScheme::Doi,
            CatalogPublicationIdentifierScheme::Oclc => SyncScheme::Oclc,
            CatalogPublicationIdentifierScheme::Lccn => SyncScheme::Lccn,
            CatalogPublicationIdentifierScheme::Google => SyncScheme::Google,
            CatalogPublicationIdentifierScheme::Goodreads => SyncScheme::Goodreads,
            CatalogPublicationIdentifierScheme::Kobo => SyncScheme::Kobo,
        };
        let identifier = sync_common::PublicationIdentifier::new(&self.value, scheme, Vec::new()).ok()?;
        match self.scheme {
            CatalogPublicationIdentifierScheme::Isbn | CatalogPublicationIdentifierScheme::Doi => identifier.canonical_value(),
            _ => Some(identifier.lookup_value().trim().to_lowercase()),
        }
    }

    pub fn is_valid_search_identifier(&self) -> bool {
        !self.value.trim().is_empty() && self.value.len() <= 512 && !self.value.chars().any(char::is_control) && self.normalized_value().is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogAuthorDiscoveryRequest {
    pub author_name: String,
    pub book_identifiers: Vec<CatalogPublicationIdentifier>,
}

#[derive(Clone, Debug, PartialEq, Eq, Type)]
#[specta(type = CatalogWideSearchOutcomeFrontend)]
pub enum CatalogWideSearchOutcome {
    Results { page: CatalogPage },
    AuthenticationRequired,
    Unsupported,
    TimedOut,
    Failed { message: String },
}

#[derive(Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[specta(inline, collect = false)]
enum CatalogWideSearchOutcomeFrontend {
    Results { page: CatalogPage },
    AuthenticationRequired,
    Unsupported,
    TimedOut,
    Failed { message: String },
}

impl From<&CatalogWideSearchOutcome> for CatalogWideSearchOutcomeFrontend {
    fn from(value: &CatalogWideSearchOutcome) -> Self {
        match value {
            CatalogWideSearchOutcome::Results { page } => Self::Results { page: page.clone() },
            CatalogWideSearchOutcome::AuthenticationRequired => Self::AuthenticationRequired,
            CatalogWideSearchOutcome::Unsupported => Self::Unsupported,
            CatalogWideSearchOutcome::TimedOut => Self::TimedOut,
            CatalogWideSearchOutcome::Failed { message } => Self::Failed { message: message.clone() },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogWideSearchOutcome", tag = "kind", rename_all = "snake_case")]
enum CatalogWideSearchOutcomeHuman {
    Results { page: CatalogPage },
    AuthenticationRequired,
    Unsupported,
    TimedOut,
    Failed { message: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogWideSearchOutcome")]
enum CatalogWideSearchOutcomeBinary {
    Results { page: CatalogPage },
    AuthenticationRequired,
    Unsupported,
    TimedOut,
    Failed { message: String },
}

dual_serde!(CatalogWideSearchOutcome, CatalogWideSearchOutcomeHuman, CatalogWideSearchOutcomeBinary);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogPagination {
    pub first: Option<String>,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub last: Option<String>,
    pub number_of_items: Option<u32>,
    pub items_per_page: Option<u32>,
    pub current_page: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogBookFormat {
    Epub,
    Pdf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogFreshness {
    Current,
    Stale,
}

impl CatalogBookFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Pdf => "PDF",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogPrice {
    pub currency: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct CatalogPublicationIdentity {
    pub edition_id: String,
    pub work_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogImportRequest {
    pub source_id: CatalogSourceId,
    pub catalog_locator: String,
    pub publication: CatalogPublicationIdentity,
    pub download_url: String,
    pub book_format: CatalogBookFormat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogExternalActionKind {
    Open,
    Preview,
    Buy,
    Borrow,
    Subscribe,
}

#[derive(Clone, Debug, PartialEq, Eq, Type)]
#[specta(type = CatalogPublicationActionFrontend)]
pub enum CatalogPublicationAction {
    Import { request: CatalogImportRequest },
    External { purpose: CatalogExternalActionKind, url: String, price: Option<CatalogPrice>, available_formats: Vec<CatalogBookFormat> },
}

#[derive(Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[specta(inline, collect = false)]
enum CatalogPublicationActionFrontend {
    Import { request: CatalogImportRequest },
    External { purpose: CatalogExternalActionKind, url: String, price: Option<CatalogPrice>, available_formats: Vec<CatalogBookFormat> },
}

impl From<&CatalogPublicationAction> for CatalogPublicationActionFrontend {
    fn from(value: &CatalogPublicationAction) -> Self {
        match value {
            CatalogPublicationAction::Import { request } => Self::Import { request: request.clone() },
            CatalogPublicationAction::External { purpose, url, price, available_formats } => Self::External { purpose: *purpose, url: url.clone(), price: price.clone(), available_formats: available_formats.clone() },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogPublicationAction", tag = "kind", rename_all = "snake_case")]
enum CatalogPublicationActionHuman {
    Import { request: CatalogImportRequest },
    External { purpose: CatalogExternalActionKind, url: String, price: Option<CatalogPrice>, available_formats: Vec<CatalogBookFormat> },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogPublicationAction")]
enum CatalogPublicationActionBinary {
    Import { request: CatalogImportRequest },
    External { purpose: CatalogExternalActionKind, url: String, price: Option<CatalogPrice>, available_formats: Vec<CatalogBookFormat> },
}

dual_serde!(CatalogPublicationAction, CatalogPublicationActionHuman, CatalogPublicationActionBinary);

#[derive(Clone, Debug, PartialEq, Eq, Type)]
#[specta(type = CatalogDescriptionFrontend)]
pub enum CatalogDescription {
    PlainText { text: String },
    RichHtml { safe_html: String, plain_text: String },
}

#[derive(Serialize, Deserialize, Type)]
#[serde(tag = "format", rename_all = "snake_case")]
#[specta(inline, collect = false)]
enum CatalogDescriptionFrontend {
    PlainText { text: String },
    RichHtml { safe_html: String, plain_text: String },
}

impl From<&CatalogDescription> for CatalogDescriptionFrontend {
    fn from(value: &CatalogDescription) -> Self {
        match value {
            CatalogDescription::PlainText { text } => Self::PlainText { text: text.clone() },
            CatalogDescription::RichHtml { safe_html, plain_text } => Self::RichHtml { safe_html: safe_html.clone(), plain_text: plain_text.clone() },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogDescription", tag = "format", rename_all = "snake_case")]
enum CatalogDescriptionHuman {
    PlainText { text: String },
    RichHtml { safe_html: String, plain_text: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogDescription")]
enum CatalogDescriptionBinary {
    PlainText { text: String },
    RichHtml { safe_html: String, plain_text: String },
}

dual_serde!(CatalogDescription, CatalogDescriptionHuman, CatalogDescriptionBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogAuthorCredit {
    pub name: String,
    pub sort_name: Option<String>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<CatalogLinkTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogPublication {
    pub identity: CatalogPublicationIdentity,
    pub title: String,
    pub authors: Vec<CatalogAuthorCredit>,
    pub description: Option<CatalogDescription>,
    pub image_url: Option<String>,
    pub actions: Vec<CatalogPublicationAction>,
}

#[derive(Clone, Debug, PartialEq, Eq, Type)]
#[specta(type = CatalogAuthorDiscoveryEvidenceFrontend)]
pub enum CatalogAuthorDiscoveryEvidence {
    BookIdentifier { identifier: CatalogPublicationIdentifier },
    AuthorName,
}

#[derive(Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[specta(inline, collect = false)]
enum CatalogAuthorDiscoveryEvidenceFrontend {
    BookIdentifier { identifier: CatalogPublicationIdentifier },
    AuthorName,
}

impl From<&CatalogAuthorDiscoveryEvidence> for CatalogAuthorDiscoveryEvidenceFrontend {
    fn from(value: &CatalogAuthorDiscoveryEvidence) -> Self {
        match value {
            CatalogAuthorDiscoveryEvidence::BookIdentifier { identifier } => Self::BookIdentifier { identifier: identifier.clone() },
            CatalogAuthorDiscoveryEvidence::AuthorName => Self::AuthorName,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthorDiscoveryEvidence", tag = "kind", rename_all = "snake_case")]
enum CatalogAuthorDiscoveryEvidenceHuman {
    BookIdentifier { identifier: CatalogPublicationIdentifier },
    AuthorName,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthorDiscoveryEvidence")]
enum CatalogAuthorDiscoveryEvidenceBinary {
    BookIdentifier { identifier: CatalogPublicationIdentifier },
    AuthorName,
}

dual_serde!(CatalogAuthorDiscoveryEvidence, CatalogAuthorDiscoveryEvidenceHuman, CatalogAuthorDiscoveryEvidenceBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogAuthorDiscoveryMatch {
    pub source: CatalogSource,
    pub publication: CatalogPublication,
    pub evidence: CatalogAuthorDiscoveryEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogAuthorDiscoveryResult {
    pub author_name: String,
    pub matches: Vec<CatalogAuthorDiscoveryMatch>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogGroup {
    pub title: String,
    pub page_locator: Option<String>,
    pub number_of_items: Option<u32>,
    pub navigation: Vec<CatalogLink>,
    pub publications: Vec<CatalogPublication>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogFacet {
    pub title: String,
    pub links: Vec<CatalogLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogPage {
    pub source_id: CatalogSourceId,
    pub freshness: CatalogFreshness,
    pub title: String,
    pub locator: String,
    pub navigation: Vec<CatalogLink>,
    pub publications: Vec<CatalogPublication>,
    pub groups: Vec<CatalogGroup>,
    pub facets: Vec<CatalogFacet>,
    pub pagination: CatalogPagination,
    pub search: Option<CatalogSearch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogOwnershipStatus {
    NotOwned,
    OtherEditionOwned,
    ExactEditionOwned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogPublicationOwnership {
    pub edition_id: String,
    pub status: CatalogOwnershipStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_frontend_shape<T, U>(runtime: &T, frontend: U)
    where
        T: Serialize,
        U: Serialize,
    {
        assert_eq!(serde_json::to_value(runtime).unwrap(), serde_json::to_value(frontend).unwrap());
    }

    #[test]
    fn generated_frontend_catalog_shapes_match_runtime_json() {
        let authentication = CatalogAuthentication::Basic { username_label: "Email".to_owned(), password: CatalogPasswordRequirement::Required };
        assert_frontend_shape(&authentication, CatalogAuthenticationFrontend::from(&authentication));

        let link = CatalogLinkTarget::Page { locator: "new".to_owned() };
        assert_frontend_shape(&link, CatalogLinkTargetFrontend::from(&link));

        let search = CatalogSearchDefinition::OpdsUriTemplate { base_url: "https://catalog.example".to_owned(), template: "{?searchTerms}".to_owned() };
        assert_frontend_shape(&search, CatalogSearchDefinitionFrontend::from(&search));

        let outcome = CatalogWideSearchOutcome::Failed { message: "offline".to_owned() };
        assert_frontend_shape(&outcome, CatalogWideSearchOutcomeFrontend::from(&outcome));

        let action = CatalogPublicationAction::External { purpose: CatalogExternalActionKind::Open, url: "https://publisher.example/book".to_owned(), price: None, available_formats: vec![CatalogBookFormat::Epub] };
        assert_frontend_shape(&action, CatalogPublicationActionFrontend::from(&action));

        let description = CatalogDescription::RichHtml { safe_html: "<p>Book</p>".to_owned(), plain_text: "Book".to_owned() };
        assert_frontend_shape(&description, CatalogDescriptionFrontend::from(&description));

        let evidence = CatalogAuthorDiscoveryEvidence::BookIdentifier { identifier: CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: "9780306406157".to_owned() } };
        assert_frontend_shape(&evidence, CatalogAuthorDiscoveryEvidenceFrontend::from(&evidence));
    }

    #[test]
    fn link_targets_cannot_mix_page_and_external_destinations() {
        let page = CatalogLinkTarget::Page { locator: "https://catalog.example/new".to_owned() };
        let external = CatalogLinkTarget::External { url: "https://publisher.example/book".to_owned() };
        assert_eq!(serde_json::to_value(page).unwrap(), serde_json::json!({"kind":"page","locator":"https://catalog.example/new"}));
        assert_eq!(serde_json::to_value(external).unwrap(), serde_json::json!({"kind":"external","url":"https://publisher.example/book"}));
    }

    #[test]
    fn source_ids_reject_protocol_and_path_data() {
        assert!(CatalogSourceId::parse("overdrive").is_ok());
        assert!(CatalogSourceId::parse("https://example.com").is_err());
        assert!(CatalogSourceId::parse("../catalog").is_err());
    }

    #[test]
    fn discovery_identifiers_compare_canonical_isbn_and_doi_values() {
        let compact_isbn = CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: "9780306406157".to_owned() };
        let urn_isbn = CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: "urn:isbn:978-0-306-40615-7".to_owned() };
        assert_eq!(compact_isbn.normalized_value(), urn_isbn.normalized_value());

        let doi = CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Doi, value: "https://doi.org/10.1000/Example".to_owned() };
        assert_eq!(doi.normalized_value().as_deref(), Some("10.1000/example"));

        let invalid = CatalogPublicationIdentifier { scheme: CatalogPublicationIdentifierScheme::Isbn, value: "not an isbn".to_owned() };
        assert!(!invalid.is_valid_search_identifier());
    }
}
