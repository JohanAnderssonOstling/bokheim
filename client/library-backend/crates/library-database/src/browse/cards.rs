//! The one card query. Browse queries decide which books a page shows and in
//! what order, and return their ids; this loads what a card shows for them.
use library_model::BookCardRow;

use super::{CardsSql, read_book_card_row};
use crate::{Database, DatabaseError};

impl Database {
    /// Cards for `ids`, in the order given. Every id must name a book: callers
    /// pass ids chosen inside the same browse snapshot, so they can pair the
    /// cards with per-row data by position.
    pub(crate) fn load_cards(&self, ids: &[i64]) -> Result<Vec<BookCardRow>, DatabaseError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let book_ids = serde_json::to_string(ids).map_err(DatabaseError::operation)?;
        let mut cards = Vec::with_capacity(ids.len());
        self.connection.load_book_cards(&book_ids, |row| {
            cards.push(read_book_card_row(row)?);
            Ok(())
        })?;
        if cards.len() != ids.len() {
            return Err(DatabaseError::message("a browsed book disappeared before its card was loaded"));
        }
        Ok(cards)
    }
}
