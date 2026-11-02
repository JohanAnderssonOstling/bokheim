//! Validation and discovery of an OPDS source, without application state.
use client_runtime::BackendError;
use library_model::{OpdsCredentials, OpdsSource};
use url::Url;

pub async fn validated_source(url: String, name: Option<String>, credentials: Option<&OpdsCredentials>) -> Result<Option<OpdsSource>, BackendError> {
    http_response::run(async move {
        let url = Url::parse(url.trim()).map_err(BackendError::operation)?;
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
            return Err("use an HTTP or HTTPS feed URL and supply credentials separately".into());
        }
        let Some(title) = crate::feed_title(url.as_str(), credentials).await? else { return Ok(None) };
        let name = name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()).unwrap_or(title);
        Ok(Some(OpdsSource { id: uuid::Uuid::new_v4(), name, url: url.into() }))
    })
    .await
    .map_err(BackendError::operation)?
}
