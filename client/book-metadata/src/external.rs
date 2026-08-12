//! Common contracts for transient metadata obtained from external catalogs.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

pub type ExternalMetadataFuture<'a> = Pin<Box<dyn Future<Output = Result<ExternalMetadataLookup, ExternalMetadataError>> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExternalMetadataProviderId(String);

impl ExternalMetadataProviderId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalClassificationScheme {
    Lcc,
    Ddc,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExternalClassification {
    pub scheme: ExternalClassificationScheme,
    pub code: String,
    pub edition: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalSubjectScheme {
    Lcsh,
    Lcgft,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExternalSubject {
    pub scheme: ExternalSubjectScheme,
    pub components: Vec<String>,
    pub authority_uri: Option<String>,
}

impl ExternalSubject {
    pub fn display_path(&self) -> String {
        self.components.join(" / ")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExternalMetadataRecord {
    pub provider_record_id: Option<String>,
    pub title: Option<String>,
    pub isbns: Vec<String>,
    pub classifications: Vec<ExternalClassification>,
    pub subjects: Vec<ExternalSubject>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalMetadataLookup {
    pub provider: ExternalMetadataProviderId,
    pub queried_isbns: Vec<String>,
    pub records: Vec<ExternalMetadataRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalMetadataError(String);

impl ExternalMetadataError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ExternalMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ExternalMetadataError {}

pub trait ExternalMetadataProvider: Send + Sync {
    fn id(&self) -> ExternalMetadataProviderId;
    fn lookup_isbn<'a>(&'a self, isbn: &'a str) -> ExternalMetadataFuture<'a>;
}

#[derive(Debug)]
pub struct ExternalProviderOutcome {
    pub provider: ExternalMetadataProviderId,
    pub result: Result<ExternalMetadataLookup, ExternalMetadataError>,
}

/// A collection of independently queried providers. A failure from one source
/// is returned alongside successful results from the others.
#[derive(Default)]
pub struct ExternalMetadataProviders {
    providers: Vec<Box<dyn ExternalMetadataProvider>>,
}

impl ExternalMetadataProviders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, provider: impl ExternalMetadataProvider + 'static) {
        self.providers.push(Box::new(provider));
    }

    pub fn with(mut self, provider: impl ExternalMetadataProvider + 'static) -> Self {
        self.add(provider);
        self
    }

    pub fn provider_ids(&self) -> Vec<ExternalMetadataProviderId> {
        self.providers.iter().map(|provider| provider.id()).collect()
    }

    pub async fn lookup_isbn(&self, isbn: &str) -> Vec<ExternalProviderOutcome> {
        let mut outcomes = Vec::with_capacity(self.providers.len());
        for provider in &self.providers {
            let id = provider.id();
            outcomes.push(ExternalProviderOutcome { provider: id, result: provider.lookup_isbn(isbn).await });
        }
        outcomes
    }

    pub async fn lookup_isbn_with(&self, provider_id: &ExternalMetadataProviderId, isbn: &str) -> Option<ExternalProviderOutcome> {
        let provider = self.providers.iter().find(|provider| provider.id() == *provider_id)?;
        Some(ExternalProviderOutcome { provider: provider_id.clone(), result: provider.lookup_isbn(isbn).await })
    }
}
