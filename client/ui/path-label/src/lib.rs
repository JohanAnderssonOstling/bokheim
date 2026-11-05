//! Compact, display-only path formatting shared by applications.
//! File operations always use the original path.

use std::path::Path;

const MAX_PATH_LABEL_CHARS: usize = 64;

/// A compact path for labels, tooltips, progress, and reports on every platform.
/// Windows device prefixes are hidden, the current user's home becomes `~`,
/// and long paths lose their middle while preserving their beginning and end.
/// The result must never be parsed back into a filesystem path.
pub fn path_label(path: &Path) -> String {
    let mut label = readable_path(&path.to_string_lossy());
    let home_variable = if cfg!(target_os = "windows") { "USERPROFILE" } else { "HOME" };
    if let Some(home) = std::env::var_os(home_variable) {
        let home = readable_path(&home.to_string_lossy());
        let home = home.trim_end_matches(['/', '\\']);
        let windows_path = label.as_bytes().get(1) == Some(&b':') || label.starts_with(r"\\");
        let absolute_home = home.starts_with('/') || home.starts_with(r"\\") || home.as_bytes().get(1) == Some(&b':');
        let matches_home = absolute_home && label.get(..home.len()).is_some_and(|prefix| if windows_path { prefix.eq_ignore_ascii_case(home) } else { prefix == home });
        if matches_home && let Some(suffix) = label.get(home.len()..) {
            if suffix.is_empty() || suffix.starts_with(['/', '\\']) {
                label = format!("~{suffix}");
            }
        }
    }
    if label.chars().count() <= MAX_PATH_LABEL_CHARS {
        return label;
    }

    // Keep the filename whole when it fits, with room left for the root.
    let filename_chars = label.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default().chars().count();
    let suffix_chars = filename_chars.clamp((MAX_PATH_LABEL_CHARS - 1) / 2, MAX_PATH_LABEL_CHARS - 12);
    let prefix_chars = MAX_PATH_LABEL_CHARS - suffix_chars - 1;
    let prefix_end = label.char_indices().nth(prefix_chars).map_or(label.len(), |(offset, _)| offset);
    let suffix_start = label.char_indices().rev().nth(suffix_chars - 1).map_or(0, |(offset, _)| offset);
    format!("{}…{}", &label[..prefix_end], &label[suffix_start..])
}

fn readable_path(path: &str) -> String {
    // This also handles Windows paths presented by a browser or another OS.
    // Removing a device prefix is safe here because this is only a label.
    let device_path = path.strip_prefix(r"\\?\").or_else(|| path.strip_prefix(r"\\.\"));
    let path = device_path.unwrap_or(path);
    let path = if device_path.is_some() && path.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"UNC\")) { format!(r"\\{}", &path[4..]) } else { path.to_owned() };
    if path.as_bytes().get(1) == Some(&b':') || path.starts_with(r"\\") { path.replace('/', "\\") } else { path }
}
