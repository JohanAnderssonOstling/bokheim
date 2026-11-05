//! Preserve known recovery causes while adding the phase of book opening.

use client_protocol::{BackendError, Failure};

pub(crate) fn opening_error(error: BackendError, stage: &str, fallback: Failure) -> BackendError {
    if error.failure().is_some() { error.context(format!("stage={stage}")) } else { BackendError::classified(fallback, format!("stage={stage}: {error}")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn known_storage_and_network_causes_take_precedence_over_opening_stage() {
        for failure in [Failure::StorageFull, Failure::PermissionDenied, Failure::Offline, Failure::SignInRequired] {
            let error = opening_error(BackendError::classified(failure, "specific cause"), "book_open", Failure::InvalidBookContainer);
            assert_eq!(error.failure(), Some(failure));
            assert!(error.to_string().contains("stage=book_open"));
        }
        let error = opening_error(BackendError::operation(io::Error::from(io::ErrorKind::StorageFull)), "content_prepare", Failure::InvalidBookContent);
        assert_eq!(error.failure(), Some(Failure::StorageFull));
    }

    #[test]
    fn unknown_errors_get_stage_specific_recovery_and_keep_diagnostics_across_serialization() {
        let error = opening_error(BackendError::message("OPS/chapter2.html: invalid markup"), "document_prepare", Failure::BookDocumentFailed);
        assert_eq!(error.failure(), Some(Failure::BookDocumentFailed));
        let restored: BackendError = serde_json::from_str(&serde_json::to_string(&error).unwrap()).unwrap();
        assert_eq!(restored.failure(), error.failure());
        assert_eq!(restored.to_string(), error.to_string());
        assert!(restored.to_string().contains("OPS/chapter2.html"));
    }
}
