//! Persistent semantic relationships between books in a library.

use crate::ContentHash;
use crate::app::BackendError;
use crate::library::LibraryBackend;
pub use app_contract::semantic_links::BookLink;
use rusqlite::Connection;

use include_sqlite_sql::{impl_sql, include_sql};

include_sql!("src/library/database/sql/book_links.sql");

impl LibraryBackend {
    pub fn book_links(&self, content_hash: ContentHash) -> Result<Vec<BookLink>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        load_book_links(&conn, content_hash).map_err(BackendError::operation)
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

fn load_book_links(conn: &Connection, content_hash: ContentHash) -> Result<Vec<BookLink>, Box<dyn std::error::Error>> {
    let mut links = Vec::new();
    conn.get_book_links(content_hash.as_str(), |row| {
        links.push(BookLink {
            id: row.get(0)?,
            source_content_hash: ContentHash::new(&row.get::<_, String>(1)?),
            target_content_hash: ContentHash::new(&row.get::<_, String>(2)?),
            relation: row.get(3)?,
            note: row.get(4)?,
            created_at: row.get(5)?,
            modified_at: row.get(6)?,
        });
        Ok(())
    })?;
    Ok(links)
}

fn store_book_link(conn: &Connection, value: &BookLink) -> Result<(), Box<dyn std::error::Error>> {
    value.validate(false)?;
    let mut source_exists = false;
    conn.live_book_exists(value.source_content_hash.as_str(), |row| {
        source_exists = row.get(0)?;
        Ok(())
    })?;
    let mut target_exists = false;
    conn.live_book_exists(value.target_content_hash.as_str(), |row| {
        target_exists = row.get(0)?;
        Ok(())
    })?;
    if !source_exists || !target_exists {
        return Err("both books in a semantic link must exist in the library".into());
    }
    conn.upsert_book_link(&value.id, value.source_content_hash.as_str(), value.target_content_hash.as_str(), &value.relation, &value.note, value.created_at, value.modified_at)?;
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
        value = Some(BookLink {
            id: row.get(0)?,
            source_content_hash: ContentHash::new(&row.get::<_, String>(1)?),
            target_content_hash: ContentHash::new(&row.get::<_, String>(2)?),
            relation: row.get(3)?,
            note: row.get(4)?,
            created_at: row.get(5)?,
            modified_at: row.get(6)?,
        });
        Ok(())
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_model::BookLinkState;
    use library_replica::{MutationBody, StateMutation};
    use sync_common::{LibraryRevision, MutationId, ReplicaSeq, ServerMutation};
    use test_support::fixture_content_hash;

    fn database() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::library::database::init_db(&connection).unwrap();
        connection.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES(?1, 'First', 1, 'epub'),(?2, 'Second', 2, 'epub')", [fixture_content_hash(1).as_str(), fixture_content_hash(2).as_str()]).unwrap();
        connection
    }

    #[test]
    fn link_mutations_are_queryable_and_create_syncable_tombstones() {
        let connection = database();
        let link = BookLink {
            id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(),
            source_content_hash: fixture_content_hash(1),
            target_content_hash: fixture_content_hash(2),
            relation: "builds on".to_owned(),
            note: "Compare chapter three".to_owned(),
            created_at: 10,
            modified_at: 10,
        };
        store_book_link(&connection, &link).unwrap();
        assert_eq!(load_book_links(&connection, fixture_content_hash(1)).unwrap(), vec![link.clone()]);
        assert_eq!(load_book_links(&connection, fixture_content_hash(2)).unwrap(), vec![link.clone()]);

        remove_book_link(&connection, &link.id, 11).unwrap();
        assert!(load_book_links(&connection, fixture_content_hash(1)).unwrap().is_empty());
        let matching = crate::library::database::get_pending_mutations(&connection).unwrap().into_iter().filter(|mutation| matches!(&mutation.body, MutationBody::BookLink { link_id, value } if link_id == &link.id && value.deleted)).count();
        assert_eq!(matching, 1);
    }

    #[test]
    fn rejects_self_links_and_missing_books() {
        let connection = database();
        let mut link = BookLink {
            id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(),
            source_content_hash: fixture_content_hash(1),
            target_content_hash: fixture_content_hash(1),
            relation: "related to".to_owned(),
            note: String::new(),
            created_at: 10,
            modified_at: 10,
        };
        assert!(store_book_link(&connection, &link).is_err());
        link.target_content_hash = fixture_content_hash(3);
        assert!(store_book_link(&connection, &link).is_err());
    }

    fn remote_link(link: &BookLink, deleted: bool, physical_time: u64, revision: u64) -> ServerMutation {
        ServerMutation {
            mutation: StateMutation {
                mutation_id: MutationId::new(),
                body: MutationBody::BookLink {
                    link_id: link.id.clone(),
                    value: BookLinkState {
                        source_content_hash: link.source_content_hash.clone(),
                        target_content_hash: link.target_content_hash.clone(),
                        relation: link.relation.clone(),
                        note: link.note.clone(),
                        created_at: u64::try_from(link.created_at).unwrap(),
                        modified_at: u64::try_from(link.modified_at).unwrap(),
                        deleted,
                    },
                },
                changed_at: physical_time,
                replica_seq: ReplicaSeq::new(revision).unwrap(),
            }
            .to_wire()
            .unwrap(),
            replica_id: test_support::fixture_replica_id("remote"),
            revision: LibraryRevision::new(revision).unwrap(),
        }
    }

    #[test]
    fn remote_link_tombstone_rejects_an_out_of_order_update() {
        let mut connection = database();
        let link = BookLink {
            id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(),
            source_content_hash: fixture_content_hash(1),
            target_content_hash: fixture_content_hash(2),
            relation: "answers".to_owned(),
            note: String::new(),
            created_at: 10,
            modified_at: 20,
        };
        let transaction = connection.transaction().unwrap();
        crate::library::database::apply_remote_change_in_tx(&transaction, &remote_link(&link, true, 20, 2)).unwrap();
        crate::library::database::apply_remote_change_in_tx(&transaction, &remote_link(&link, false, 10, 1)).unwrap();
        transaction.commit().unwrap();

        assert!(load_book_links(&connection, fixture_content_hash(1)).unwrap().is_empty());
        let deleted: bool = connection.query_row("SELECT deleted_at IS NOT NULL FROM book_link WHERE id=?1", [&link.id], |row| row.get(0)).unwrap();
        assert!(deleted);
    }
}
