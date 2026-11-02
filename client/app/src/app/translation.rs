//! User-requested translation, sent directly from the device to MyMemory.
use std::time::Duration;

use super::core::AppBackend;
use crate::Url;
use serde_json::Value;

pub const MAX_SELECTION_BYTES: usize = 500;
pub const SELECTION_TOO_LONG: &str = "Selection is too long. Select up to 500 UTF-8 bytes to translate.";

/// Normalize book tags and system locales without discarding script or region.
pub fn language_code(tag: &str) -> Option<String> {
    let tag = tag.trim().split(['.', '@']).next()?.replace('_', "-");
    let mut parts = tag.split('-');
    let mut code = dictionary_catalog::language(parts.next()?)?;
    for part in parts {
        if part.is_empty() || part.len() > 8 || !part.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return None;
        }
        code.push('-');
        code.push_str(&part.to_ascii_lowercase());
    }
    Some(code)
}

impl AppBackend {
    pub(crate) async fn translate_selection(&self, text: &str, source: &str, target: &str) -> Result<String, String> {
        // Rust strings contain UTF-8 bytes. Check the original selection before
        // URL encoding, including whitespace, and never truncate or split it.
        if text.len() > MAX_SELECTION_BYTES {
            return Err(SELECTION_TOO_LONG.into());
        }
        if text.trim().is_empty() {
            return Err("Select some text to translate.".into());
        }
        let source = language_code(source).ok_or("Choose the source language.")?;
        let target = language_code(target).ok_or("Choose a translation language.")?;
        if source == target {
            return Ok(text.into());
        }
        let client = self
            .state
            .translation_client
            .get_or_init(|| async {
                let builder = http_response::client_builder();
                #[cfg(not(target_arch = "wasm32"))]
                let builder = builder.retry(reqwest::retry::never());
                builder.build().ok()
            })
            .await
            .as_ref()
            .ok_or("Could not start translation.")?;
        let mut url = Url::parse("https://api.mymemory.translated.net/get").expect("fixed translation URL");
        url.query_pairs_mut().append_pair("q", text).append_pair("langpair", &format!("{source}|{target}"));
        let response = client.get(url).timeout(Duration::from_secs(10)).send().await.map_err(|_| "Could not reach MyMemory. Check your connection and try again.")?;
        if response.status().as_u16() == 429 {
            return Err("MyMemory's translation limit has been reached. Try again later.".into());
        }
        let response = response.error_for_status().map_err(|_| "MyMemory could not translate this selection. Try again later.")?;
        let bytes = http_response::read(response, 64 * 1024).await.map_err(|_| "Could not read the translation response.")?;
        let response: Value = serde_json::from_slice(&bytes).map_err(|_| "MyMemory returned an unreadable translation.")?;
        if response.get("quotaFinished").and_then(Value::as_bool) == Some(true) {
            return Err("MyMemory's daily translation limit has been reached. Try again tomorrow.".into());
        }
        let status = &response["responseStatus"];
        if status.as_u64() != Some(200) && status.as_str() != Some("200") {
            return Err(response["responseDetails"].as_str().filter(|value| !value.trim().is_empty()).unwrap_or("MyMemory could not translate this language pair.").into());
        }
        response.pointer("/responseData/translatedText").and_then(Value::as_str).filter(|text| !text.trim().is_empty()).map(str::to_owned).ok_or_else(|| "MyMemory returned no translation.".into())
    }
}
