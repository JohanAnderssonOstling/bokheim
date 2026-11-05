//! Persistent website storage for browser windows.
use anyhow::Result;
use std::path::Path;
use wry::WebContext;

pub(crate) fn context(directory: &Path) -> Result<WebContext> {
    std::fs::create_dir_all(directory)?;
    Ok(WebContext::new(Some(directory.to_owned())))
}

#[cfg(target_os = "linux")]
pub(crate) fn prepare(page: &wry::WebView, context: &WebContext) -> Result<()> {
    use anyhow::Context;
    use webkit2gtk::{CookieManagerExt, CookiePersistentStorage, WebContextExt, WebViewExt};
    use wry::WebViewExtUnix;

    let directory = context.data_directory().context("browser profile has no persistent storage directory")?;
    let manager = page.webview().context().and_then(|context| context.cookie_manager()).context("browser cookie storage is unavailable")?;
    manager.set_persistent_storage(&directory.join("cookies.sqlite").to_string_lossy(), CookiePersistentStorage::Sqlite);
    Ok(())
}
