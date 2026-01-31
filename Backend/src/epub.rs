use epub_provider::EpubProvider;
use std::path::{Path, PathBuf};

pub fn get_epub_id_and_title(path: &Path) -> Option<(String, String)> {
    let provider = EpubProvider::new(PathBuf::from(&path));
    let metadata = provider.metadata().ok()?;
    let title = metadata.title;
    let id = "".to_string();
    Some((id, title))
}
