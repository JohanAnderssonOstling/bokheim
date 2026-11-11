//! User-requested translation, sent directly from the device to MyMemory.
use std::time::Duration;
use ui_text::text;

use crate::{LookupError, Services};
use operation_failure::{Classified, Failure};
use language_tags::LanguageTag;
use serde_json::Value;
use url::Url;

pub const MAX_SELECTION_BYTES: usize = 500;
pub const SELECTION_TOO_LONG: &str = text!(reader::translation_selection_too_long);

#[derive(Clone, Debug)]
pub enum TranslationError {
    TooLong,
    Empty,
    Request(LookupError),
    RateLimited,
    DailyLimit,
    UnsupportedLanguages,
    EmptyResponse,
}
impl std::fmt::Display for TranslationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(error) => std::fmt::Display::fmt(error, formatter),
            error => write!(formatter, "{error:?}"),
        }
    }
}
impl std::error::Error for TranslationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self { Self::Request(error) => Some(error), _ => None }
    }
}
impl Classified for TranslationError {
    fn failure(&self) -> Option<Failure> {
        match self {
            Self::Request(error) => error.failure(),
            Self::RateLimited | Self::DailyLimit => Some(Failure::RateLimited),
            _ => None,
        }
    }
}

impl Services {
    pub(crate) async fn translate_selection(&self, text: &str, source: &LanguageTag, target: &LanguageTag) -> Result<String, TranslationError> {
        // Rust strings contain UTF-8 bytes. Check the original selection before
        // URL encoding, including whitespace, and never truncate or split it.
        if text.len() > MAX_SELECTION_BYTES {
            return Err(TranslationError::TooLong);
        }
        if text.trim().is_empty() {
            return Err(TranslationError::Empty);
        }
        if source == target {
            return Ok(text.into());
        }
        http_response::run(async {
            let client = self
                .translation_client
                .get_or_try_init(|| async {
                    let builder = http_response::client_builder();
                    #[cfg(not(target_arch = "wasm32"))]
                    let builder = builder.retry(reqwest::retry::never());
                    builder.build().map_err(LookupError::from)
                })
                .await
                .map_err(TranslationError::Request)?;
            let mut url = Url::parse("https://api.mymemory.translated.net/get").expect("fixed translation URL");
            url.query_pairs_mut().append_pair("q", text).append_pair("langpair", &format!("{source}|{target}"));
            let response = client.get(url).timeout(Duration::from_secs(10)).send().await.map_err(|error| TranslationError::Request(error.into()))?;
            if response.status().as_u16() == 429 {
                return Err(TranslationError::RateLimited);
            }
            let response = response.error_for_status().map_err(|error| TranslationError::Request(LookupError::operation(error)))?;
            let bytes = http_response::read(response, 64 * 1024).await.map_err(|error| TranslationError::Request(LookupError::operation(error)))?;
            let response: Value = serde_json::from_slice(&bytes).map_err(|error| TranslationError::Request(LookupError::operation(error)))?;
            if response.get("quotaFinished").and_then(Value::as_bool) == Some(true) {
                return Err(TranslationError::DailyLimit);
            }
            let status = &response["responseStatus"];
            if status.as_u64() != Some(200) && status.as_str() != Some("200") {
                log::warn!("Translation service rejected the request with status {status}");
                return Err(TranslationError::UnsupportedLanguages);
            }
            response.pointer("/responseData/translatedText").and_then(Value::as_str).filter(|text| !text.trim().is_empty()).map(str::to_owned).ok_or(TranslationError::EmptyResponse)
        })
        .await
        .map_err(|error| TranslationError::Request(LookupError::operation(error)))?
    }
}
