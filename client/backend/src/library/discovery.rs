//! Optional filesystem discovery and its application-event adapter.
use super::events::{LibraryEvent, LibraryEventSender};
use crate::storage::files::AssetStore;
use library_runtime::database::Database;

pub(crate) enum LibraryScanner {
    #[cfg(feature = "scanner")]
    Files(library_files::scanner::LibraryScanner<Database>),
    Disabled,
}
impl LibraryScanner {
    pub(crate) fn from_root(root: &str, database: Database, assets: AssetStore, sender: LibraryEventSender) -> std::io::Result<Self> {
        #[cfg(feature = "scanner")]
        {
            let events = library_files::events::ScanEventSink::new(move |event| {
                use library_files::events::ScanEvent;
                let _ = sender.try_send(match event {
                    ScanEvent::ScanStarted => LibraryEvent::ScanStarted,
                    ScanEvent::ScanComplete => LibraryEvent::ScanComplete,
                    ScanEvent::TransferStatusChanged => LibraryEvent::TransferStatusChanged,
                });
            });
            library_files::scanner::LibraryScanner::from_root(root, database, assets, events).map(Self::Files)
        }
        #[cfg(not(feature = "scanner"))]
        {
            let _ = (root, database, assets, sender);
            Ok(Self::Disabled)
        }
    }
    pub(crate) fn enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }
    pub(crate) fn progress(&self) -> Option<crate::ScanProgress> {
        match self {
            #[cfg(feature = "scanner")]
            Self::Files(scanner) => scanner.is_scanning().then(|| scanner.scan_progress()),
            Self::Disabled => None,
        }
    }
    pub(crate) fn operation_scanning(&self) -> bool {
        match self {
            #[cfg(feature = "scanner")]
            Self::Files(scanner) => scanner.has_started() && scanner.is_scanning(),
            Self::Disabled => false,
        }
    }
    pub(crate) fn start_scanning(&self) {
        match self {
            #[cfg(feature = "scanner")]
            Self::Files(scanner) => scanner.start_scanning(),
            Self::Disabled => {}
        }
    }
}

pub(crate) fn estimate_folders(locators: Vec<String>, cancelled: &std::sync::atomic::AtomicBool) -> Result<(u64, u64, u64), crate::BackendError> {
    #[cfg(feature = "scanner")]
    {
        library_files::scanner::estimate_folders_cancellable(locators.into_iter().map(std::path::PathBuf::from).collect(), || cancelled.load(std::sync::atomic::Ordering::Acquire))
            .ok_or_else(|| crate::BackendError::message("Folder estimate cancelled"))
    }
    #[cfg(not(feature = "scanner"))]
    {
        let _ = (locators, cancelled);
        Err(crate::BackendError::message("Folder size estimation is unavailable on this device"))
    }
}
