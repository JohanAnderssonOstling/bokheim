//! Import failure text, independent of notification widgets.

use ui_text::text;

use library_backend::ImportFailure;
pub(crate) fn import_failure_message(failure: &ImportFailure) -> String {
    let summary = import_failure_summary(failure);
    if failure.failure.is_some() { format!(text!(browse::import_summary), ui_components::book_transfer_error_message(failure.failure), summary = summary) } else { summary }
}

fn import_failure_summary(failure: &ImportFailure) -> String {
    use library_backend::ImportFailureKind;
    match failure.kind {
        ImportFailureKind::File => match failure.path.last() {
            Some(name) => format!(text!(browse::could_not_import), name = name),
            None => text!(browse::could_not_import_a_book).into(),
        },
        ImportFailureKind::Folder => {
            if let Some(hidden) = failure.path.iter().position(|part| part.starts_with('.')) {
                return match hidden.checked_sub(1).and_then(|i| failure.path.get(i)) {
                    Some(parent) => format!(text!(browse::a_folder_in_could_not_be_imported), parent = parent),
                    None => text!(browse::a_folder_could_not_be_imported).into(),
                };
            }
            match failure.path.last() {
                Some(name) => format!(text!(browse::could_not_import_folder), name = name),
                None => text!(browse::a_folder_could_not_be_imported).into(),
            }
        }
    }
}
