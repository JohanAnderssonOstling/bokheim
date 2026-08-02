use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use epub_svelte_lib::BookId;
use epub_svelte_lib::library::Book;
use gpui::{Context, Image, ImageFormat, SharedString};

use crate::model::BookCard;
use crate::{BrowserView, COVER_CACHE_BYTES, COVER_CACHE_LIMIT};

const COVER_LOAD_BATCH: usize = 24;

impl BrowserView {
    pub(crate) fn advance_cover_generation(&mut self) {
        self.cover_generation = self.cover_generation.wrapping_add(1);
        self.pending_cover_ids.clear();
        self.loading_cover_ids.clear();
        self.cover_load_running = false;
    }

    pub(crate) fn cards_from_books(&mut self, books: Vec<Book>, show_progress: bool) -> Vec<BookCard> {
        let mut cards = Vec::with_capacity(books.len());
        for book in books {
            let book_id = book.id();
            let cover = match self.cover_cache.get(&book_id) {
                Some(cover) => Some(cover.clone()),
                None => {
                    if let Some(bytes) = book.cover() {
                        let cost = cover_memory_cost(&bytes);
                        let cover = Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes.to_vec()));
                        self.remember_cover(book_id, cover.clone(), cost);
                        Some(cover)
                    } else {
                        None
                    }
                }
            };
            let author = book.author().map(|author| author.as_str()).unwrap_or("Unknown author");
            cards.push(BookCard {
                id: book_id,
                title: book.title().as_str().to_owned().into(),
                author: author.to_owned().into(),
                cover,
                progress: show_progress.then(|| SharedString::from(format!("Progress {:.0}%", book.read_progress().value()))),
            });
        }
        self.trim_cover_cache();
        cards
    }

    pub(crate) fn queue_cover_load(&mut self, book_ids: impl IntoIterator<Item = BookId>, cx: &mut Context<Self>) {
        for book_id in book_ids {
            if !self.cover_cache.contains_key(&book_id) && !self.missing_cover_ids.contains(&book_id) && !self.loading_cover_ids.contains(&book_id) {
                self.pending_cover_ids.insert(book_id);
            }
        }
        self.start_cover_batch(cx);
    }

    fn start_cover_batch(&mut self, cx: &mut Context<Self>) {
        if self.cover_load_running || self.pending_cover_ids.is_empty() {
            return;
        }
        let book_ids = self.pending_cover_ids.iter().take(COVER_LOAD_BATCH).copied().collect::<Vec<_>>();
        for book_id in &book_ids {
            self.pending_cover_ids.remove(book_id);
            self.loading_cover_ids.insert(*book_id);
        }

        let generation = self.cover_generation;
        let Some(library_id) = self.active_library_id.clone() else {
            return;
        };
        let app_core = self.app_core.clone();
        self.cover_load_running = true;
        let loading_ids = book_ids.clone();
        let load_covers = self.blocking_worker.dispatch(move || {
            book_ids
                .into_iter()
                .map(|book_id| {
                    let cover = (|| {
                        app_core.thumbnail_in(&library_id, book_id).map_err(|error| error.to_string()).map(|cover| {
                            cover.map(|bytes| {
                                let cost = cover_memory_cost(&bytes);
                                (bytes, cost)
                            })
                        })
                    })();
                    (book_id, cover)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let Ok(covers) = load_covers.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.cover_generation != generation {
                    return;
                }
                for book_id in loading_ids {
                    this.loading_cover_ids.remove(&book_id);
                }
                this.cover_load_running = false;
                if this.install_covers(covers) {
                    cx.notify();
                }
                this.start_cover_batch(cx);
            });
        })
        .detach();
    }

    pub(crate) fn queue_visible_cover_reload(&mut self, book_id: BookId, cx: &mut Context<Self>) {
        if !self.has_visible_book(book_id) {
            return;
        }
        self.pending_thumbnail_ids.insert(book_id);
        if self.thumbnail_reload_scheduled {
            return;
        }
        self.thumbnail_reload_scheduled = true;
        let timer = cx.background_executor().timer(Duration::from_millis(50));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                this.thumbnail_reload_scheduled = false;
                let book_ids = std::mem::take(&mut this.pending_thumbnail_ids);
                for book_id in &book_ids {
                    this.forget_cover(*book_id);
                    this.missing_cover_ids.remove(book_id);
                }
                this.queue_cover_load(book_ids, cx);
            });
        })
        .detach();
    }

    fn install_covers(&mut self, covers: Vec<(BookId, Result<Option<(Vec<u8>, usize)>, String>)>) -> bool {
        if covers.is_empty() {
            return false;
        }

        let mut changed_ids = HashSet::with_capacity(covers.len());
        for (book_id, result) in covers {
            let cover = match result {
                Ok(cover) => cover,
                Err(error) => {
                    log::error!("failed to load thumbnail for book {book_id}: {error}");
                    self.missing_cover_ids.insert(book_id);
                    continue;
                }
            };
            let Some((bytes, cost)) = cover else {
                self.missing_cover_ids.insert(book_id);
                continue;
            };
            let cover = Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes));
            self.remember_cover(book_id, cover, cost);
            changed_ids.insert(book_id);
        }
        self.trim_cover_cache();

        let mut changed = false;
        if let Some(cards) = Self::patch_cards(&self.books, &self.cover_cache, &changed_ids) {
            self.books = Arc::new(cards);
            changed = true;
        }

        let mut home_sections = None;
        for (index, section) in self.home_sections.iter().enumerate() {
            let Some(cards) = Self::patch_cards(&section.books, &self.cover_cache, &changed_ids) else {
                continue;
            };
            let sections = home_sections.get_or_insert_with(|| self.home_sections.as_ref().clone());
            sections[index].books = Arc::new(cards);
        }
        if let Some(sections) = home_sections {
            self.home_sections = Arc::new(sections);
            changed = true;
        }

        let mut author_groups = None;
        for (index, group) in self.author_groups.iter().enumerate() {
            let Some(cards) = Self::patch_cards(&group.books, &self.cover_cache, &changed_ids) else {
                continue;
            };
            let groups = author_groups.get_or_insert_with(|| self.author_groups.as_ref().clone());
            groups[index].books = Arc::new(cards);
        }
        if let Some(groups) = author_groups {
            self.author_groups = Arc::new(groups);
            changed = true;
        }

        changed
    }

    fn patch_cards(cards: &[BookCard], cache: &HashMap<BookId, Arc<Image>>, changed_ids: &HashSet<BookId>) -> Option<Vec<BookCard>> {
        if !cards.iter().any(|card| changed_ids.contains(&card.id)) {
            return None;
        }
        Some(
            cards
                .iter()
                .cloned()
                .map(|mut card| {
                    if changed_ids.contains(&card.id) {
                        card.cover = cache.get(&card.id).cloned();
                    }
                    card
                })
                .collect(),
        )
    }

    fn has_visible_book(&self, book_id: BookId) -> bool {
        self.books.iter().any(|card| card.id == book_id)
            || self.home_sections.iter().any(|section| section.books.iter().any(|card| card.id == book_id))
            || self.author_groups.iter().any(|group| group.books.iter().any(|card| card.id == book_id))
    }

    fn remember_cover(&mut self, book_id: BookId, cover: Arc<Image>, cost: usize) {
        self.remove_cached_cover(book_id);
        self.cover_cache.insert(book_id, cover);
        self.cover_costs.insert(book_id, cost);
        self.cover_cache_bytes = self.cover_cache_bytes.saturating_add(cost);
        self.cover_order.push_back(book_id);
    }

    fn forget_cover(&mut self, book_id: BookId) {
        self.remove_cached_cover(book_id);
        self.clear_card_covers(&HashSet::from([book_id]));
    }

    fn remove_cached_cover(&mut self, book_id: BookId) {
        self.cover_cache.remove(&book_id);
        self.cover_order.retain(|cached_id| *cached_id != book_id);
        if let Some(cost) = self.cover_costs.remove(&book_id) {
            self.cover_cache_bytes = self.cover_cache_bytes.saturating_sub(cost);
        }
    }

    fn trim_cover_cache(&mut self) {
        let mut evicted = HashSet::new();
        while self.cover_order.len() > COVER_CACHE_LIMIT || self.cover_cache_bytes > COVER_CACHE_BYTES {
            if let Some(book_id) = self.cover_order.pop_front() {
                self.cover_cache.remove(&book_id);
                evicted.insert(book_id);
                if let Some(cost) = self.cover_costs.remove(&book_id) {
                    self.cover_cache_bytes = self.cover_cache_bytes.saturating_sub(cost);
                }
            }
        }
        self.clear_card_covers(&evicted);
    }

    fn clear_card_covers(&mut self, evicted: &HashSet<BookId>) {
        if evicted.is_empty() {
            return;
        }
        if let Some(cards) = clear_cards(&self.books, evicted) {
            self.books = Arc::new(cards);
        }
        let mut home_sections = None;
        for (index, section) in self.home_sections.iter().enumerate() {
            let Some(cards) = clear_cards(&section.books, evicted) else {
                continue;
            };
            let sections = home_sections.get_or_insert_with(|| self.home_sections.as_ref().clone());
            sections[index].books = Arc::new(cards);
        }
        if let Some(sections) = home_sections {
            self.home_sections = Arc::new(sections);
        }
        let mut author_groups = None;
        for (index, group) in self.author_groups.iter().enumerate() {
            let Some(cards) = clear_cards(&group.books, evicted) else {
                continue;
            };
            let groups = author_groups.get_or_insert_with(|| self.author_groups.as_ref().clone());
            groups[index].books = Arc::new(cards);
        }
        if let Some(groups) = author_groups {
            self.author_groups = Arc::new(groups);
        }
    }
}

fn clear_cards(cards: &[BookCard], evicted: &HashSet<BookId>) -> Option<Vec<BookCard>> {
    if !cards.iter().any(|card| card.cover.is_some() && evicted.contains(&card.id)) {
        return None;
    }
    Some(
        cards
            .iter()
            .cloned()
            .map(|mut card| {
                if evicted.contains(&card.id) {
                    card.cover = None;
                }
                card
            })
            .collect(),
    )
}

fn cover_memory_cost(bytes: &[u8]) -> usize {
    image::io::Reader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .map(|(width, height)| (width as usize).saturating_mul(height as usize).saturating_mul(4))
        .unwrap_or(bytes.len())
        .max(bytes.len())
}
