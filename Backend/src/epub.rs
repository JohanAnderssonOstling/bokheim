use epub_provider::EpubProvider;
use std::fs::File;
use std::path::Path;

pub fn get_epub_id_title_author(path: &Path) -> Option<(String, String, String)> {
    let provider = match File::open(path).and_then(EpubProvider::try_from_reader) {
        Ok(provider) => provider,
        Err(err) => {
            log::warn!("Skipping EPUB metadata read for {}: {}", path.display(), err);
            return None;
        }
    };
    let metadata = provider.metadata().ok()?;
    let title = metadata.title;
    let author = metadata
        .creators
        .iter()
        .find(|creator| creator.role == epub_provider::CreatorRole::Author)
        .or_else(|| metadata.creators.first())
        .map(|creator| creator.name.clone())
        .unwrap_or_default();
    let id = "".to_string();
    Some((id, title, author))
}
