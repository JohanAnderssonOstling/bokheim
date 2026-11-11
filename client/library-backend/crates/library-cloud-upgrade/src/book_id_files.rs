//! Prepare new asset names before committing their database bindings. Old names
//! remain intact for rollback; the ordinary collector retires them afterwards.
use super::{Result, invalid};
use content_address::{BookId, ContentHash};
use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

fn regular_length(path: &Path) -> Result<Option<u64>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
        Ok(_) => Err(invalid(format!("asset is not a regular file: {}", path.display()))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn link(source: &Path, destination: &Path) -> Result<()> {
    if regular_length(destination)?.is_some() {
        return Ok(());
    }
    let directory = destination.parent().ok_or_else(|| invalid("asset has no parent directory"))?;
    fs::create_dir_all(directory)?;
    fs::hard_link(source, destination)?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

pub(super) fn prepare(ids: &BTreeMap<BookId, BookId>, blobs: &[(BookId, ContentHash, u64)], apply: bool) -> Result<()> {
    let root = std::env::var_os("BOKHEIM_ASSETS_DIR").map(PathBuf::from);
    let books = root.as_ref().map(|root| root.join("books")).unwrap_or_else(|| PathBuf::from("books"));
    let thumbnails = root.as_ref().map(|root| root.join("thumbnails")).unwrap_or_else(|| PathBuf::from("thumbnails"));
    let pins = std::env::var_os("BOKHEIM_CATALOG_PINS_DIR").map(PathBuf::from);
    let book_path = |book: BookId| books.join(&book.to_hex()[..2]).join(book.to_hex());
    let mut links = Vec::new();
    for &(old, checksum, size) in blobs {
        let Some(&new) = ids.get(&old) else { continue };
        let destination = book_path(new);
        if let Some(length) = regular_length(&destination)? {
            if length != size {
                return Err(invalid("existing migrated book has a different byte length"));
            }
            continue;
        }
        let canonical = BookId::from(checksum);
        let source = [Some(book_path(old)), Some(book_path(canonical)), pins.as_ref().map(|pins| pins.join(canonical.to_hex()))]
            .into_iter()
            .flatten()
            .find_map(|path| match regular_length(&path) {
                Ok(Some(length)) => Some(if length == size { Ok(path) } else { Err(invalid("stored book has a different byte length")) }),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .ok_or_else(|| invalid(format!("missing stored bytes for {old}; use the server's asset-directory configuration")))??;
        // Retained catalog pins may be on another filesystem. The server can
        // already resolve those by the unchanged canonical checksum.
        if pins.as_ref().is_some_and(|pins| source.starts_with(pins)) {
            continue;
        }
        links.push((source, destination));
    }
    match fs::read_dir(thumbnails.join("users")) {
        Ok(users) => {
            for user in users {
                let user = user?;
                if !user.file_type()?.is_dir() {
                    continue;
                }
                for file in fs::read_dir(user.path())? {
                    let file = file?;
                    let name = file.file_name();
                    let Some(name) = name.to_str() else { continue };
                    let Some((stem, suffix)) = [".pair", "-300.jpg", ".jpg"].into_iter().find_map(|suffix| name.strip_suffix(suffix).map(|stem| (stem, suffix))) else { continue };
                    let Some(new) = stem.parse::<BookId>().ok().and_then(|old| ids.get(&old)) else { continue };
                    regular_length(&file.path())?;
                    links.push((file.path(), user.path().join(format!("{new}{suffix}"))));
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if apply {
        for (source, destination) in links {
            link(&source, &destination)?;
        }
    }
    Ok(())
}
