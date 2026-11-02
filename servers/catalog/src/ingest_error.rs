//! Acquisition recovery depends on the failed operation, never its message.
use reqwest::{Error as HttpError, StatusCode, Url};
use std::{error::Error, fmt};

#[derive(Debug)]
pub(crate) enum IngestError {
    Fetch { url: Url, error: HttpError },
    Source(String),
    InvalidBook(String),
    Enrichment(String),
    Alternative { primary: Box<Self>, alternative: Box<Self> },
    Other(String),
}

impl IngestError {
    pub fn can_try_alternative(&self) -> bool {
        matches!(self, Self::Fetch { .. } | Self::Source(_) | Self::InvalidBook(_))
    }

    pub fn needs_source_pause(&self) -> bool {
        match self {
            Self::Fetch { error, .. } => error.is_timeout() || matches!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE)),
            Self::Alternative { primary, alternative } => primary.needs_source_pause() || alternative.needs_source_pause(),
            _ => false,
        }
    }
}

impl fmt::Display for IngestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch { url, error } => write!(formatter, "{url}: {error}"),
            Self::Enrichment(message) => write!(formatter, "enrichment failed, add again later: {message}"),
            Self::Alternative { primary, alternative } => write!(formatter, "{primary}; linked EPUB also failed: {alternative}"),
            Self::Source(message) | Self::InvalidBook(message) | Self::Other(message) => formatter.write_str(message),
        }
    }
}

impl Error for IngestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Fetch { error, .. } => Some(error),
            Self::Alternative { alternative, .. } => Some(alternative.as_ref()),
            _ => None,
        }
    }
}

impl From<String> for IngestError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for IngestError {
    fn from(message: &str) -> Self {
        Self::Other(message.to_owned())
    }
}

// CLI and persisted diagnostics are the boundary where recovery information
// can be discarded. Ingestion retains the typed error until policy is applied.
impl From<IngestError> for String {
    fn from(error: IngestError) -> Self {
        error.to_string()
    }
}
