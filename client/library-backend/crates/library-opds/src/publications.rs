//! The publications this session has listed, by their card key: what adding
//! one or drawing its cover needs, which a card does not carry. Like the
//! page table, it lives as long as the process.
use crate::fetch::{self, Access, Failure};
use crate::{Publication, PDF};
use library_model::{CardKey, OpdsCredentials, OpdsSource};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use url::Url;

struct Listed {
    file_stem: String,
    download: Option<(Url, &'static str)>,
    cover: Option<Url>,
    description: String,
}

static LISTED: LazyLock<Mutex<HashMap<CardKey, Listed>>> = LazyLock::new(Default::default);
/// Covers as JPEG thumbnails, by card and width; `None` for one that could
/// not be had. A failed fetch is not remembered.
static COVERS: LazyLock<Mutex<HashMap<(CardKey, u32), Option<Vec<u8>>>>> = LazyLock::new(Default::default);

pub(crate) fn remember(key: CardKey, publication: &Publication) {
    let name = publication.title.replace(['/', '\\', ':'], " ");
    let file_stem = if publication.authors.is_empty() { name } else { format!("{} - {name}", publication.authors.join(", ")) };
    let listed = Listed { file_stem: file_stem.trim().to_owned(), download: publication.download.as_ref().map(|download| (download.href.clone(), download.media_type)), cover: publication.cover.clone(), description: publication.description.clone() };
    LISTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, listed);
}

fn listed<T>(key: CardKey, read: impl FnOnce(&Listed) -> T) -> Option<T> {
    LISTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key).map(read)
}

/// The feed's description of a listed publication, as HTML: what its page
/// shows. Read from the listing, so it costs no request.
pub fn description(key: CardKey) -> Result<String, String> {
    listed(key, |listed| listed.description.clone()).ok_or_else(|| "this book is no longer listed; open its page again".to_owned())
}

/// The publication's open download, with the file name to import it under.
pub async fn download(source: &OpdsSource, credentials: Option<&OpdsCredentials>, key: CardKey) -> Result<(String, Vec<u8>), String> {
    let (file_stem, (href, media_type)) = listed(key, |listed| Some((listed.file_stem.clone(), listed.download.clone()?))).ok_or("this book is no longer listed; open its page again")?.ok_or("this book has no open download")?;
    let root = Url::parse(&source.url).map_err(|error| error.to_string())?;
    let bytes = match fetch::bytes(&href, &Access::new(&root, credentials)).await {
        Ok(bytes) => bytes,
        Err(Failure::SignIn) => return Err("the feed asks to sign in".into()),
        Err(Failure::Other(error)) => return Err(error),
    };
    let extension = if media_type == PDF { "pdf" } else { "epub" };
    Ok((format!("{file_stem}.{extension}"), bytes))
}

/// The publication's cover as a JPEG `width` pixels wide, or none when it
/// has none or it cannot be read.
pub async fn cover(source: &OpdsSource, credentials: Option<&OpdsCredentials>, key: CardKey, width: u32) -> Option<Vec<u8>> {
    if let Some(cover) = COVERS.lock().ok()?.get(&(key, width)) {
        return cover.clone();
    }
    let href = listed(key, |listed| listed.cover.clone())??;
    let root = Url::parse(&source.url).ok()?;
    let bytes = fetch::bytes(&href, &Access::new(&root, credentials)).await.ok()?;
    let cover = thumbnail_jpeg(&bytes, width);
    COVERS.lock().ok()?.insert((key, width), cover.clone());
    cover
}

fn thumbnail_jpeg(bytes: &[u8], width: u32) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    let decoded = thumbnail::decode_cover_image(bytes, format).ok()?;
    thumbnail::encode_thumbnail_at_width(&decoded, width).ok()
}
