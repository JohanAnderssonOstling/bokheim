//! A catalog import's window-owned progress subscription. Subscribe before the
//! operation starts so even a fast response cannot race the first update.

use super::format::download_byte_progress;
use futures_util::future::{Either, select};
use gpui::AsyncWindowContext;
use library_backend::LibraryClient;
use library_model::{CardKey, CatalogImportProgress, LibraryUpdate};
use std::future::Future;
use ui_components::{BrowserDownloadPhase, BrowserDownloadUpdate, BrowserDownloads};

pub(crate) async fn track_catalog_import<T>(library: &LibraryClient, entry: CardKey, download: &str, downloads: &mut BrowserDownloads, cx: &mut AsyncWindowContext, operation: impl Future<Output = T>) -> T {
    let updates = match library.updates().await {
        Ok((updates, _)) => updates,
        Err(error) => {
            log::warn!("Could not subscribe to catalog import progress: {error}");
            return operation.await;
        }
    };
    futures_util::pin_mut!(operation);
    loop {
        let update = match select(operation.as_mut(), Box::pin(updates.recv())).await {
            Either::Left((result, _)) => return result,
            Either::Right((Ok(update), _)) => update,
            Either::Right((Err(_), _)) => return operation.await,
        };
        let LibraryUpdate::CatalogImport { entry: key, progress } = update else { continue };
        if key != entry {
            continue;
        }
        let _ = cx.update(|window, cx| match progress {
            CatalogImportProgress::Downloading { downloaded_bytes, total_bytes } => {
                let (percent, text) = download_byte_progress(downloaded_bytes, total_bytes);
                downloads.progress_detail(download, percent, text.into(), window, cx);
            }
            CatalogImportProgress::Waiting => downloads.waiting(download, window, cx),
            CatalogImportProgress::Importing => {
                let Some(active) = downloads.get(download) else { return };
                let name = active.name.clone();
                downloads.apply(BrowserDownloadUpdate { download: download.into(), replaces: None, name, phase: BrowserDownloadPhase::Importing, percent: None, message: None }, window, cx);
            }
        });
    }
}
