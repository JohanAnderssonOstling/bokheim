//! User-requested translation, sent directly from the device to MyMemory.
use std::time::Duration;
use ui_text::text;

use crate::Services;
use language_tags::LanguageTag;
use serde_json::Value;
use url::Url;

pub const MAX_SELECTION_BYTES: usize = 500;
pub const SELECTION_TOO_LONG: &str = text!(reader::translation_selection_too_long);

impl Services {
    pub(crate) async fn translate_selection(&self, text: &str, source: &LanguageTag, target: &LanguageTag) -> Result<String, String> {
        // Rust strings contain UTF-8 bytes. Check the original selection before
        // URL encoding, including whitespace, and never truncate or split it.
        if text.len() > MAX_SELECTION_BYTES {
            return Err(SELECTION_TOO_LONG.into());
        }
        if text.trim().is_empty() {
            return Err(text!(reader::select_some_text_to_translate).into());
        }
        if source == target {
            return Ok(text.into());
        }
        http_response::run(async {
            let client = self
                .translation_client
                .get_or_init(|| async {
                    let builder = http_response::client_builder();
                    #[cfg(not(target_arch = "wasm32"))]
                    let builder = builder.retry(reqwest::retry::never());
                    builder.build().ok()
                })
                .await
                .as_ref()
                .ok_or(text!(reader::translation_start_failed))?;
            let mut url = Url::parse("https://api.mymemory.translated.net/get").expect("fixed translation URL");
            url.query_pairs_mut().append_pair("q", text).append_pair("langpair", &format!("{source}|{target}"));
            let response = client.get(url).timeout(Duration::from_secs(10)).send().await.map_err(|_| text!(reader::translation_connection_failed))?;
            if response.status().as_u16() == 429 {
                return Err(text!(reader::translation_limit_reached).into());
            }
            let response = response.error_for_status().map_err(|_| text!(reader::translation_failed))?;
            let bytes = http_response::read(response, 64 * 1024).await.map_err(|_| text!(reader::translation_failed))?;
            let response: Value = serde_json::from_slice(&bytes).map_err(|_| text!(reader::translation_failed))?;
            if response.get("quotaFinished").and_then(Value::as_bool) == Some(true) {
                return Err(text!(reader::translation_daily_limit_reached).into());
            }
            let status = &response["responseStatus"];
            if status.as_u64() != Some(200) && status.as_str() != Some("200") {
                log::warn!("Translation service rejected the request with status {status}");
                return Err(text!(reader::translation_languages_unsupported).into());
            }
            response.pointer("/responseData/translatedText").and_then(Value::as_str).filter(|text| !text.trim().is_empty()).map(str::to_owned).ok_or_else(|| text!(reader::translation_empty).into())
        })
        .await
        .map_err(|_| text!(reader::translation_start_failed))?
    }
}
