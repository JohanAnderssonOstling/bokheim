use super::*;
use crate::Failure;

#[test]
fn folder_step_and_cause_survive_worker_serialization() {
    let failure =
        LibraryFolderError { folder_name: Some("My Library".into()), step: LibraryFolderStep::SaveLibraryInfo, error: BackendError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).context("saving library identity") };
    let bytes = client_runtime::wire::encode_worker_message(&failure).unwrap();
    let restored: LibraryFolderError = client_runtime::wire::decode_worker_message(&bytes).unwrap();
    assert_eq!(restored.folder_name.as_deref(), Some("My Library"));
    assert_eq!(restored.step, LibraryFolderStep::SaveLibraryInfo);
    assert_eq!(restored.error.failure(), Some(Failure::PermissionDenied));
}
