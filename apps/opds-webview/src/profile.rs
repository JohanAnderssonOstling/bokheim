//! Persistent website storage shared by the browser and feed verification.
use anyhow::Result;
use library_opds::browser::shared_profile;
use std::path::{Path, PathBuf};
use wry::{WebContext, WebView};

// Also accept requests from desktop executables that still send a per-origin
// path, so replacing just the helper does not create a separate session.
fn directory(requested: &Path) -> PathBuf {
    if requested.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())) {
        if let Some(parent) = requested.parent() {
            let root = if parent.file_name().is_some_and(|name| name == "stores") { parent.parent().unwrap_or(parent) } else { parent };
            return shared_profile(root);
        }
    }
    requested.to_owned()
}

pub(crate) fn context(requested: &Path) -> Result<WebContext> {
    let directory = directory(requested);
    std::fs::create_dir_all(&directory)?;
    Ok(WebContext::new(Some(directory)))
}

pub(crate) fn prepare(page: &WebView, context: &WebContext) -> Result<()> {
    #[cfg(target_os = "linux")]
    linux::prepare(page, context.data_directory().ok_or_else(|| anyhow::anyhow!("browser profile has no persistent storage directory"))?)?;
    #[cfg(not(target_os = "linux"))]
    let _ = (page, context);
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anyhow::{Context, anyhow};
    use gtk::glib::MainContext;
    use soup::prelude::CookieJarExt;
    use soup::{Cookie, CookieJarText};
    use std::collections::HashMap;
    use std::fs::{self, File};
    use webkit2gtk::{CookieManagerExt, CookiePersistentStorage, WebContextExt, WebViewExt};
    use wry::WebViewExtUnix;

    pub(super) fn prepare(page: &WebView, directory: &Path) -> Result<()> {
        // Serialize the one-time import, rather than locking the profile for
        // the lifetime of a window. SQLite writes individual cookie changes;
        // parallel helpers cannot overwrite a whole text jar with stale data.
        let lock = File::options().read(true).write(true).create(true).truncate(false).open(directory.join("cookie-import.lock"))?;
        lock.lock()?;
        let manager = page.webview().context().and_then(|context| context.cookie_manager()).ok_or_else(|| anyhow!("browser cookie storage is unavailable"))?;
        manager.set_persistent_storage(&directory.join("cookies.sqlite").to_string_lossy(), CookiePersistentStorage::Sqlite);
        let marker = directory.join("cookies-imported");
        if !marker.exists() {
            for mut cookie in legacy_cookies(directory)? {
                MainContext::default().block_on(manager.add_cookie_future(&mut cookie)).context("could not preserve a saved browser cookie")?;
            }
            fs::write(marker, b"")?;
        }
        Ok(())
    }

    fn legacy_cookies(directory: &Path) -> Result<Vec<Cookie>> {
        let mut sources = Vec::new();
        if let Some(root) = directory.parent().and_then(Path::parent).filter(|root| shared_profile(root) == directory) {
            for parent in [root.to_owned(), root.join("stores")] {
                if !parent.is_dir() {
                    continue;
                }
                for entry in fs::read_dir(parent)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path != directory && entry.file_type()?.is_dir() && path.join("cookies").is_file() {
                        sources.push(path.join("cookies"));
                    }
                }
            }
        }
        // Newer legacy profiles win conflicts; the existing globe profile has
        // final priority so its most recent completed verification is retained.
        sources.sort_by_key(|path| fs::metadata(path).and_then(|metadata| metadata.modified()).ok());
        if directory.join("cookies").is_file() {
            sources.push(directory.join("cookies"));
        }
        let mut cookies = HashMap::new();
        for source in sources {
            // Surface an unreadable file rather than marking a partial import
            // complete. Soup preserves HttpOnly, Secure, expiry and SameSite.
            File::open(&source)?;
            let jar = CookieJarText::new(&source.to_string_lossy(), true);
            for mut cookie in jar.all_cookies() {
                let key = (cookie.domain(), cookie.path(), cookie.name());
                cookies.insert(key, cookie);
            }
        }
        Ok(cookies.into_values().collect())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn imports_legacy_cookies_without_losing_attributes_or_restoring_expired_cookies() {
            let root = tempfile::tempdir().unwrap();
            let directory = shared_profile(root.path());
            let old = root.path().join("stores").join("a".repeat(64));
            fs::create_dir_all(&directory).unwrap();
            fs::create_dir_all(&old).unwrap();
            fs::write(old.join("cookies"), "#HttpOnly_.example.com\tTRUE\t/\tTRUE\t1893456000\tverified\tolder\tStrict\n.example.org\tTRUE\t/\tFALSE\t1893456000\tsignin\tkept\tLax\n.example.org\tTRUE\t/\tFALSE\t1\texpired\tdeleted\tLax\n")
                .unwrap();
            fs::write(directory.join("cookies"), "#HttpOnly_.example.com\tTRUE\t/\tTRUE\t1893456000\tverified\tcurrent\tStrict\n").unwrap();
            let mut cookies = legacy_cookies(&directory).unwrap();
            assert_eq!(cookies.len(), 2);
            let index = cookies.iter_mut().position(|cookie| cookie.name().as_deref() == Some("verified")).unwrap();
            let verified = &mut cookies[index];
            assert_eq!(verified.value().as_deref(), Some("current"));
            assert!(verified.is_http_only());
            assert!(verified.is_secure());
            assert_eq!(verified.same_site_policy(), soup::SameSitePolicy::Strict);
            assert_eq!(verified.expires().unwrap().to_unix(), 1893456000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_store_and_verification_requests_share_the_original_globe_profile() {
        let root = Path::new("profiles");
        assert_eq!(directory(&root.join("stores").join("a".repeat(64))), shared_profile(root));
        assert_eq!(directory(&root.join("b".repeat(64))), shared_profile(root));
        assert_eq!(directory(&shared_profile(root)), shared_profile(root));
        assert_eq!(directory(&root.join("test-profile")), root.join("test-profile"));
    }
}
