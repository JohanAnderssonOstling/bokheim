use std::collections::HashMap;

use epub_svelte_lib::{BookLocator, ContentHash, LibraryId};
use gpui::{AppContext, BorrowAppContext, Global, WeakEntity};

use super::BookCardView;

/// UI-only routing from a book identity to the retained cards that display it.
/// Weak handles keep route changes and window teardown ownership-neutral.
#[derive(Default)]
pub(crate) struct CoverInvalidationRegistry {
    cards: HashMap<BookLocator, Vec<WeakEntity<BookCardView>>>,
}

impl Global for CoverInvalidationRegistry {}

impl CoverInvalidationRegistry {
    pub(crate) fn register<C: BorrowAppContext>(locator: BookLocator, card: WeakEntity<BookCardView>, cx: &mut C) {
        cx.update_default_global::<Self, _>(|registry, _| {
            let cards = registry.cards.entry(locator).or_default();
            cards.retain(|card| card.upgrade().is_some());
            cards.push(card);
        });
    }

    pub(crate) fn invalidate<C: BorrowAppContext + AppContext>(library_id: LibraryId, content_hashes: &[ContentHash], cx: &mut C) {
        if content_hashes.is_empty() {
            return;
        }
        cx.update_default_global::<Self, _>(|registry, cx| {
            for content_hash in content_hashes {
                let locator = BookLocator::new(library_id, content_hash.clone());
                let Some(mut cards) = registry.cards.remove(&locator) else { continue };
                cards.retain(|card| card.update(cx, |card, cx| card.thumbnail_available(cx)).is_ok());
                if !cards.is_empty() {
                    registry.cards.insert(locator, cards);
                }
            }
        });
    }
}
