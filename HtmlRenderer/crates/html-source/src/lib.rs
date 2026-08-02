use std::io;

/// A framework- and container-neutral table-of-contents entry.
#[derive(Clone, Debug, Default)]
pub struct TocEntry {
    pub title: String,
    pub link: String,
    pub children: Vec<TocEntry>,
}

/// Supplies documents and related resources to the HTML pipeline.
///
/// Implementations may read from a filesystem, an EPUB archive, a network
/// cache, or any other backing store.
pub trait ResourceProvider: Send + Sync {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>>;

    fn read_string(&self, uri: &str) -> io::Result<String> {
        let bytes = self.read_bytes(uri)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn exists(&self, uri: &str) -> bool;

    fn resolve(&self, base: &str, href: &str) -> String;

    fn list_html_candidates(&self, root: &str) -> io::Result<Vec<String>>;

    fn toc(&self) -> io::Result<Option<Vec<TocEntry>>> {
        Ok(None)
    }
}
