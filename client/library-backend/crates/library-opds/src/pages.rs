//! The pages this session has visited.
//!
//! A location names a page by a small index rather than its URL, so
//! locations stay `Copy` and cheap to compare; the table lives as long as the
//! process. A restart starts again at each feed's root.
use library_model::OpdsSourceId;
use std::sync::Mutex;
use url::Url;

static PAGES: Mutex<Vec<(OpdsSourceId, Url)>> = Mutex::new(Vec::new());

/// The index of `url` in `source`, adding it the first time it is seen.
pub(crate) fn id(source: OpdsSourceId, url: &Url) -> u32 {
    let mut pages = PAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let index = pages.iter().position(|(owner, page)| *owner == source && page == url).unwrap_or_else(|| {
        pages.push((source, url.clone()));
        pages.len() - 1
    });
    u32::try_from(index).unwrap_or(u32::MAX)
}

pub(crate) fn url(source: OpdsSourceId, page: u32) -> Option<Url> {
    let pages = PAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    pages.get(page as usize).filter(|(owner, _)| *owner == source).map(|(_, url)| url.clone())
}
