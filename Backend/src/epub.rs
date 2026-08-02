use epub_provider::EpubProvider;
use std::fs::File;
use std::path::Path;

pub fn get_epub_id_title_author(path: &Path) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    let provider = File::open(path).and_then(EpubProvider::try_from_reader)?;
    let metadata = provider.metadata()?;
    let title = metadata.title;
    let author = metadata.creators.iter().find(|creator| creator.role == epub_provider::CreatorRole::Author).or_else(|| metadata.creators.first()).map(|creator| creator.name.clone()).unwrap_or_default();
    let id = "".to_string();
    Ok((id, title, author))
}
