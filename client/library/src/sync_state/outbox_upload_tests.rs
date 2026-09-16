use super::*;
use crate::assets::book_upload_intent;
use crate::assets::complete_book_upload;
use crate::assets::observe_local_publication;
use crate::schema::init_db;
use sync_common::ContentHash;

#[test]
fn pending_upload_keeps_metadata_local_without_blocking_other_books() {
    let mut conn = database();
    let waiting = add_book(&mut conn, 1, true);
    let ready = add_book(&mut conn, 2, false);
    let original = get_pending_mutations(&conn).unwrap();

    let publishable = get_publishable_mutations(&conn).unwrap();

    assert!(!contains_book(&publishable, waiting));
    assert!(contains_book(&publishable, ready));
    assert_eq!(get_pending_mutations(&conn).unwrap(), original);
}

#[test]
fn only_the_current_upload_acknowledgement_releases_metadata() {
    let mut conn = database();
    let book = add_book(&mut conn, 1, true);
    let original = book_upload_intent(&conn, &book).unwrap().unwrap();
    let tx = conn.transaction().unwrap();
    let edited = test_support::fixture_content_hash(2);
    observe_local_publication(&tx, &book, &edited, 101).unwrap();
    tx.commit().unwrap();

    let tx = conn.transaction().unwrap();
    assert!(!complete_book_upload(&tx, &original).unwrap());
    tx.commit().unwrap();
    assert!(!contains_book(&get_publishable_mutations(&conn).unwrap(), book));

    let current = book_upload_intent(&conn, &book).unwrap().unwrap();
    let tx = conn.transaction().unwrap();
    assert!(complete_book_upload(&tx, &current).unwrap());
    tx.commit().unwrap();
    assert!(contains_book(&get_publishable_mutations(&conn).unwrap(), book));
}

#[test]
fn paged_outbox_can_pass_more_than_one_page_of_pending_uploads() {
    let mut conn = database();
    for i in 1..=300 {
        add_book(&mut conn, i, true);
    }
    let ready = add_book(&mut conn, 301, false);

    let page = pending_mutations_page(&conn, i64::MIN, outbox_watermark(&conn).unwrap()).unwrap();

    assert!(contains_book(&page, ready));
    assert!(page.iter().all(|m| m.body.upload_dependency().is_none_or(|hash| hash == ready)));
}

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn
}

fn add_book(conn: &mut Connection, number: u64, pending_upload: bool) -> ContentHash {
    let hash = test_support::fixture_content_hash(number);
    let tx = conn.transaction().unwrap();
    tx.execute(include_str!("sql/outbox_upload_tests/add_book_insert.sql"), [hash.as_str()]).unwrap();
    tx.execute(include_str!("sql/outbox_upload_tests/add_book_insert_2.sql"), rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), hash.as_str(), format!("{number}.m4b")]).unwrap();
    if pending_upload {
        observe_local_publication(&tx, &hash, &hash, 100).unwrap();
    }
    tx.commit().unwrap();
    hash
}

fn contains_book(mutations: &[StateMutation], hash: ContentHash) -> bool {
    mutations.iter().any(|mutation| mutation.body.upload_dependency() == Some(hash))
}

#[test]
fn cloud_storage_disabled_keeps_metadata_publishable_and_uploads_durable() {
    let mut conn = database();
    let book = add_book(&mut conn, 1, true);
    crate::assets::set_cloud_storage_policy(&conn, false).unwrap();
    assert!(contains_book(&get_publishable_mutations(&conn).unwrap(), book));
    assert!(contains_book(&pending_mutations_page(&conn, i64::MIN, outbox_watermark(&conn).unwrap()).unwrap(), book));
    assert!(book_upload_intent(&conn, &book).unwrap().is_some());
    crate::assets::set_cloud_storage_policy(&conn, true).unwrap();
    assert!(!contains_book(&get_publishable_mutations(&conn).unwrap(), book));
}
