//! Shared review rules for filesystem roots and provider-backed directory trees.
use gpui::SelectedDirectory;
use library_backend::BookFormat;

fn eligible(path: &[String]) -> bool {
    !path.iter().any(|part| part.starts_with('.')) && path.last().and_then(|name| std::path::Path::new(name).extension()).and_then(|ext| ext.to_str()).and_then(BookFormat::from_import_extension).is_some()
}

pub(super) fn retain_eligible_files(directory: &mut SelectedDirectory) {
    directory.directories.retain(|path| !path.iter().any(|part| part.starts_with('.')));
    directory.files.retain(|file| eligible(&file.path) && file.size_bytes != Some(0));
}

/// Path-backed roots are estimated by the backend. Provider files already carry
/// metadata, so counting them never consumes their one-shot read capabilities.
pub(super) fn estimate_selected_files(directories: &[SelectedDirectory]) -> (u64, u64, u64) {
    let (mut books, mut bytes, mut unavailable) = (0, 0u64, 0);
    for directory in directories.iter().filter(|directory| directory.local_path.is_none()) {
        for file in &directory.files {
            if !eligible(&file.path) || file.size_bytes == Some(0) {
                continue;
            }
            books += 1;
            match file.size_bytes {
                Some(size) => bytes = bytes.saturating_add(size),
                None => unavailable += 1,
            }
        }
    }
    (books, bytes, unavailable)
}

pub(super) fn estimate_label((books, bytes, unavailable): (u64, u64, u64)) -> String {
    if unavailable > 0 && bytes == 0 {
        return format!("{books} eligible books · size unavailable");
    }
    format!(
        "{books} eligible books · {}{}{}",
        if unavailable > 0 { "at least " } else { "estimated " },
        crate::services::format_storage_bytes(bytes),
        if unavailable > 0 { format!(" · {unavailable} entries could not be checked") } else { String::new() }
    )
}
