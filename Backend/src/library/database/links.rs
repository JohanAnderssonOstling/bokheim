//! Persistent semantic relationships between books in a library.

use crate::app::BackendError;
use crate::library::LibraryBackend;
use crate::BookId;
pub use bokheim_app_contract::semantic_links::BookLink;
use rusqlite::Connection;

use include_sqlite_sql::{impl_sql, include_sql};

include_sql!("src/library/database/sql/book_links.sql");

impl LibraryBackend {
    pub fn book_links(&self, book_id: BookId) -> Result<Vec<BookLink>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        load_book_links(&conn, book_id).map_err(BackendError::operation)
    }

    pub fn upsert_book_link(&self, link: &BookLink) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        store_book_link(&conn, link).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn delete_book_link(&self, link_id: &str, modified_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        remove_book_link(&conn, link_id, modified_at).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }
}

fn load_book_links(conn: &Connection, book_id: BookId) -> Result<Vec<BookLink>, Box<dyn std::error::Error>> {
    let mut links = Vec::new();
    conn.get_book_links(&book_id.canonical(), |row| {
        links.push(BookLink { id: row.get(0)?, source_book_id: row.get(1)?, target_book_id: row.get(2)?, relation: row.get(3)?, note: row.get(4)?, created_at: row.get(5)?, modified_at: row.get(6)? });
        Ok(())
    })?;
    Ok(links)
}

fn store_book_link(conn: &Connection, value: &BookLink) -> Result<(), Box<dyn std::error::Error>> {
    value.validate(false)?;
    let mut source_exists = false;
    conn.live_book_exists(&value.source_book_id, |row| {
        source_exists = row.get(0)?;
        Ok(())
    })?;
    let mut target_exists = false;
    conn.live_book_exists(&value.target_book_id, |row| {
        target_exists = row.get(0)?;
        Ok(())
    })?;
    if !source_exists || !target_exists {
        return Err("both books in a semantic link must exist in the library".into());
    }
    conn.upsert_book_link(&value.id, &value.source_book_id, &value.target_book_id, &value.relation, &value.note, value.created_at, value.modified_at)?;
    Ok(())
}

fn remove_book_link(conn: &Connection, id: &str, modified_at: i64) -> Result<(), Box<dyn std::error::Error>> {
    let mut deleted = book_link_by_id(conn, id)?.ok_or("book link not found")?;
    deleted.modified_at = modified_at;
    deleted.validate(true)?;
    conn.delete_book_link(id, modified_at)?;
    Ok(())
}

fn book_link_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<BookLink>> {
    let mut value = None;
    conn.get_book_link_by_id(id, |row| {
        value = Some(BookLink { id: row.get(0)?, source_book_id: row.get(1)?, target_book_id: row.get(2)?, relation: row.get(3)?, note: row.get(4)?, created_at: row.get(5)?, modified_at: row.get(6)? });
        Ok(())
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bokheim_domain::{BookLinkState, MutationBody, StateMutation};
    use sync_common::{ClientId, LibraryRevision, MutationId, ReplicaSeq, ServerMutation};

    fn database() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::library::database::init_db(&connection).unwrap();
        connection.execute("INSERT INTO book(id, title, added_at, format) VALUES('1', 'First', 1, 'epub'),('2', 'Second', 2, 'epub')", []).unwrap();
        connection
    }

    #[test]
    fn link_mutations_are_queryable_and_create_syncable_tombstones() {
        let connection = database();
        let link = BookLink {
            id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(),
            source_book_id: "1".to_owned(),
            target_book_id: "2".to_owned(),
            relation: "builds on".to_owned(),
            note: "Compare chapter three".to_owned(),
            created_at: 10,
            modified_at: 10,
        };
        store_book_link(&connection, &link).unwrap();
        assert_eq!(load_book_links(&connection, BookId::new(1)).unwrap(), vec![link.clone()]);
        assert_eq!(load_book_links(&connection, BookId::new(2)).unwrap(), vec![link.clone()]);

        remove_book_link(&connection, &link.id, 11).unwrap();
        assert!(load_book_links(&connection, BookId::new(1)).unwrap().is_empty());
        let matching = crate::library::database::get_pending_mutations(&connection)
            .unwrap()
            .into_iter()
            .filter(|mutation| matches!(&mutation.body, MutationBody::BookLink { link_id, value } if link_id == &link.id && value.deleted))
            .count();
        assert_eq!(matching, 1);
    }

    #[test]
    fn rejects_self_links_and_missing_books() {
        let connection = database();
        let mut link =
            BookLink { id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(), source_book_id: "1".to_owned(), target_book_id: "1".to_owned(), relation: "related to".to_owned(), note: String::new(), created_at: 10, modified_at: 10 };
        assert!(store_book_link(&connection, &link).is_err());
        link.target_book_id = "3".to_owned();
        assert!(store_book_link(&connection, &link).is_err());
    }

    fn remote_link(link: &BookLink, deleted: bool, physical_time: u64, revision: u64) -> ServerMutation {
        ServerMutation {
            mutation: StateMutation {
                mutation_id: MutationId::new(),
                body: MutationBody::BookLink {
                    link_id: link.id.clone(),
                    value: BookLinkState {
                        source_book_id: BookId::parse(&link.source_book_id).unwrap(),
                        target_book_id: BookId::parse(&link.target_book_id).unwrap(),
                        relation: link.relation.clone(),
                        note: link.note.clone(),
                        created_at: sync_common::UnixMillis::new(u64::try_from(link.created_at).unwrap()).unwrap(),
                        modified_at: sync_common::UnixMillis::new(u64::try_from(link.modified_at).unwrap()).unwrap(),
                        deleted,
                    },
                },
                changed_at: sync_common::UnixMillis::new(physical_time).unwrap(),
                replica_seq: ReplicaSeq::new(revision).unwrap(),
            }.to_wire().unwrap(),
            replica_id: ClientId::parse("remote").unwrap(),
            revision: LibraryRevision::new(revision).unwrap(),
        }
    }

    #[test]
    fn remote_link_tombstone_rejects_an_out_of_order_update() {
        let mut connection = database();
        let link = BookLink { id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(), source_book_id: "1".to_owned(), target_book_id: "2".to_owned(), relation: "answers".to_owned(), note: String::new(), created_at: 10, modified_at: 20 };
        let transaction = connection.transaction().unwrap();
        crate::library::database::apply_remote_change_in_tx(&transaction, &remote_link(&link, true, 20, 2)).unwrap();
        crate::library::database::apply_remote_change_in_tx(&transaction, &remote_link(&link, false, 10, 1)).unwrap();
        transaction.commit().unwrap();

        assert!(load_book_links(&connection, BookId::new(1)).unwrap().is_empty());
        let deleted: bool = connection.query_row("SELECT deleted_at IS NOT NULL FROM book_link WHERE id=?1", [&link.id], |row| row.get(0)).unwrap();
        assert!(deleted);
    }
}
