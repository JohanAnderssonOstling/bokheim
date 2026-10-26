//! A feed page as browse chips and cards: its navigation links are chips,
//! its publications cards, and its next page and facets locations of their
//! own.
use crate::{fetch, pages, publications, Feed, NavigationLink, Publication, PDF};
use client_runtime::{BackendError, Failure};
use library_model::{BookCardRow, BookSource, BrowseContents, BrowseLocation, BrowseRow, CardKey, FeedFacet, FeedFacetGroup, FeedPage, LibraryFileTypeFilter, OpdsCredentials, OpdsSource, OpdsSourceId};
use url::Url;

/// The page `page` of `source` (its root when `None`), or the results for
/// `search` from that page when it is not empty. A feed that refuses this
/// device is an empty page asking to sign in, not a failure.
///
/// Only this page is fetched: the pages its links lead to are fetched when
/// opened. Feeds such as Project Gutenberg's link every listed book to a
/// page of its own, and fetching those alongside held the page back.
pub async fn browse(source: &OpdsSource, credentials: Option<&OpdsCredentials>, page: Option<u32>, search: &str) -> Result<(BrowseContents, FeedPage), BackendError> {
    let root = root(&source.url)?;
    let url = match page {
        None => root.clone(),
        Some(page) => pages::url(source.id, page).ok_or_else(|| BackendError::classified(Failure::NotFound, "this feed page is no longer known"))?,
    };
    let access = fetch::Access::new(&root, credentials);
    let feed = match fetch_page(&url, search, &access).await {
        Ok(feed) => feed,
        Err(error) if error.failure() == Some(Failure::Unauthorized) => return Ok((BrowseContents::default(), FeedPage { sign_in: true, ..FeedPage::default() })),
        Err(error) => return Err(error),
    };
    let source = source.id;
    let children = feed.navigation.iter().map(|link| chip(source, link)).collect();
    let books = feed.publications.iter().map(|publication| card(source, publication)).collect();
    let location = |href| BrowseLocation::Opds { source, page: Some(pages::id(source, href)) };
    let facets = feed
        .facets
        .iter()
        .map(|group| FeedFacetGroup { title: group.title.clone(), choices: group.facets.iter().map(|facet| FeedFacet { title: facet.title.clone(), location: location(&facet.href), active: facet.active }).collect() })
        .collect();
    Ok((BrowseContents { children, books }, FeedPage { next: feed.next.as_ref().map(location), facets, sign_in: false }))
}

async fn fetch_page(url: &Url, search: &str, access: &fetch::Access<'_>) -> Result<Feed, BackendError> {
    let feed = fetch::feed(url, access).await?;
    if search.is_empty() {
        return Ok(feed);
    }
    fetch::feed(&fetch::search(&feed, search, access).await?, access).await
}

/// The title of the feed at `url`, fetching its root once; this also checks
/// that it is a feed at all, and that `credentials` let this device in.
/// Returns `None` when an anonymous request needs credentials.
pub async fn feed_title(url: &str, credentials: Option<&OpdsCredentials>) -> Result<Option<String>, BackendError> {
    let url = root(url)?;
    let feed = match fetch::feed(&url, &fetch::Access::new(&url, credentials)).await {
        Ok(feed) => feed,
        Err(error) if error.failure() == Some(Failure::Unauthorized) && credentials.is_some() => return Err(BackendError::classified(Failure::CatalogCredentialsRejected, format!("{url} refused the username and password"))),
        Err(error) if error.failure() == Some(Failure::Unauthorized) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(if feed.title.is_empty() { url.host_str().unwrap_or("OPDS feed").to_owned() } else { feed.title }))
}

fn root(url: &str) -> Result<Url, BackendError> {
    Url::parse(url.trim()).map_err(|error| BackendError::classified(Failure::InvalidAddress, format!("not a feed address: {error}")))
}

fn chip(source: OpdsSourceId, link: &NavigationLink) -> BrowseRow {
    BrowseRow { id: BrowseLocation::Opds { source, page: Some(pages::id(source, &link.href)) }, name: link.title.clone(), path: None, book_count: link.count.map_or(0, |count| count as usize), downloaded_book_count: 0 }
}

/// A publication's card; adding it or drawing its cover finds it again by
/// the card's key.
fn card(source: OpdsSourceId, publication: &Publication) -> BookCardRow {
    let entry = entry_key(source, publication);
    publications::remember(entry, publication);
    // A sold book is offered only where its store's window can open.
    #[cfg(not(target_arch = "wasm32"))]
    let buyable = publication.buy.is_some() && crate::browser::stores_available();
    #[cfg(target_arch = "wasm32")]
    let buyable = false;
    BookCardRow {
        source: BookSource::Opds { source, entry, downloadable: publication.download.is_some(), buyable },
        title: publication.title.clone(),
        subtitle: publication.subtitle.clone(),
        author: publication.authors.join(", "),
        progress: 0.0,
        downloaded: false,
        download_requested: false,
        format_category: if publication.download.as_ref().is_some_and(|download| download.media_type == PDF) { LibraryFileTypeFilter::Pdf } else { LibraryFileTypeFilter::Book },
        audiobook_duration_ms: None,
        added_at: 0,
        source_directory: None,
    }
}

/// A stable card key for a publication: its feed and OPDS id, or its title
/// where the feed gives no id.
fn entry_key(source: OpdsSourceId, publication: &Publication) -> CardKey {
    let id = if publication.id.is_empty() { &publication.title } else { &publication.id };
    CardKey(*blake3::Hasher::new().update(source.as_bytes()).update(id.as_bytes()).finalize().as_bytes())
}
