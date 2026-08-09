//! Canonical identities for synchronized binary assets.

pub use bokheim_app_contract::transfer::{BlobKey, BlobKind, TransferJobKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_values_round_trip_through_closed_enums() {
        assert_eq!(BlobKind::Book.storage(), "book");
        assert_eq!(BlobKind::Thumbnail.storage(), "thumbnail");
        for kind in [TransferJobKind::UploadBook, TransferJobKind::UploadThumbnail, TransferJobKind::DownloadBook, TransferJobKind::DownloadThumbnail] {
            assert_eq!(TransferJobKind::parse_storage(kind.storage()), Some(kind));
        }
        assert_eq!(TransferJobKind::parse_storage("upload"), None);
    }
}
