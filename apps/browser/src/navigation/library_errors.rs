use app::{Failure, LibraryFolderError, LibraryFolderStep};
use ui_components::user_facing_error;

pub(super) fn folder_add_message(failures: &[LibraryFolderError]) -> String {
    failures
        .iter()
        .map(|failure| {
            let heading = match &failure.folder_name {
                Some(name) => format!("Could not add “{name}”."),
                None => "Could not add the selected folders.".into(),
            };
            format!("{heading} {}", folder_recovery(failure))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn folder_recovery(failure: &LibraryFolderError) -> &'static str {
    let LibraryFolderError { step, error, .. } = failure;
    if *step == LibraryFolderStep::SaveLibraryInfo && matches!(error.failure(), Some(Failure::PermissionDenied | Failure::ReadOnlyStorage)) {
        log::warn!("Could not save library information: {error}");
        return "Bokheim can read this folder, but couldn't save its library information. Allow write access to the folder and Bokheim's local storage, then try again.";
    }
    let fallback = match step {
        LibraryFolderStep::CheckFolder => "Bokheim couldn't read this folder. Check that its drive is connected and that you have permission to open it.",
        LibraryFolderStep::NotAFolder => "The selected item is a file. Choose the folder containing your books instead.",
        LibraryFolderStep::ReadLibraryInfo => "This folder's saved library information couldn't be read. Update Bokheim if this library was created with a newer version, or restore its library information from a backup.",
        LibraryFolderStep::SaveLibraryInfo => "Bokheim couldn't save this folder's library information. Check that the folder and Bokheim's local storage allow changes, then try again.",
        LibraryFolderStep::OpenLibrary => "Bokheim couldn't prepare this library's local data. Restart Bokheim and try again. If it still fails, update Bokheim.",
    };
    user_facing_error(error, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use app::BackendError;

    fn failure(name: &str, step: LibraryFolderStep, cause: Option<Failure>) -> LibraryFolderError {
        LibraryFolderError {
            folder_name: Some(name.into()),
            step,
            error: cause.map(|cause| BackendError::classified(cause, "private diagnostic /home/reader/library.db")).unwrap_or_else(|| BackendError::message("private diagnostic: SQL syntax error")),
        }
    }

    #[test]
    fn unreadable_and_read_only_folders_have_different_recovery_steps() {
        let unreadable = folder_add_message(&[failure("Shared Books", LibraryFolderStep::CheckFolder, Some(Failure::PermissionDenied))]);
        let read_only = folder_add_message(&[failure("Shared Books", LibraryFolderStep::SaveLibraryInfo, Some(Failure::PermissionDenied))]);
        assert!(unreadable.contains("permission to use the folder"));
        assert!(read_only.contains("can read this folder"));
        assert!(read_only.contains("Allow write access"));
        assert!(!unreadable.contains("can read this folder"));
        assert!(!read_only.contains("/home/reader"));
    }

    #[test]
    fn failed_initialization_does_not_tell_the_user_the_folder_is_missing() {
        let message = folder_add_message(&[failure("My Library", LibraryFolderStep::OpenLibrary, None)]);
        assert!(message.contains("Could not add “My Library”"));
        assert!(message.contains("prepare this library's local data"));
        assert!(!message.contains("SQL syntax"));
        assert!(!message.contains("drive is connected"));
    }

    #[test]
    fn each_failed_folder_keeps_its_own_cause() {
        let message = folder_add_message(&[failure("Travel", LibraryFolderStep::CheckFolder, Some(Failure::LibraryFolderUnavailable)), failure("Favorites", LibraryFolderStep::SaveLibraryInfo, Some(Failure::StorageFull))]);
        assert!(message.contains("Could not add “Travel”"));
        assert!(message.contains("Reconnect its drive"));
        assert!(message.contains("Could not add “Favorites”"));
        assert!(message.contains("Free up space"));
    }
}
