use ui_text::text;

use app::{Failure, LibraryFolderError, LibraryFolderStep};
use ui_components::user_facing_error;

pub(super) fn folder_add_message(failures: &[LibraryFolderError]) -> String {
    failures
        .iter()
        .map(|failure| {
            let heading = match &failure.folder_name {
                Some(name) => format!(text!(browse::could_not_add), name = name),
                None => text!(browse::could_not_add_the_selected_folders).into(),
            };
            format!(text!(browse::heading), folder_recovery(failure), heading = heading)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn folder_recovery(failure: &LibraryFolderError) -> &'static str {
    let LibraryFolderError { step, error, .. } = failure;
    if *step == LibraryFolderStep::SaveLibraryInfo && matches!(error.failure(), Some(Failure::PermissionDenied | Failure::ReadOnlyStorage)) {
        log::warn!("Could not save library information: {error}");
        return text!(browse::the_app_can_read_this_folder_but_couldn_t_save_its_library);
    }
    let fallback = match step {
        LibraryFolderStep::CheckFolder => text!(browse::couldn_t_read_this_folder_check_its_drive_connection_and_your_access),
        LibraryFolderStep::NotAFolder => text!(browse::the_selected_item_is_a_file_choose_the_folder_containing_your_books),
        LibraryFolderStep::ReadLibraryInfo => text!(browse::couldn_t_read_this_folder_s_library_information_if_a_newer_app),
        LibraryFolderStep::SaveLibraryInfo => text!(browse::couldn_t_save_this_folder_s_library_information_check_write_access_to),
        LibraryFolderStep::OpenLibrary => text!(browse::couldn_t_prepare_this_library_s_local_data_restart_the_app_if),
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
