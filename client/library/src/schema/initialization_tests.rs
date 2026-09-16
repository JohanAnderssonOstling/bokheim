use super::*;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn fresh_schema_and_identity_commit_once() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let conn = Connection::open(&path).unwrap();
    configure_connection(&conn).unwrap();
    let commits = Arc::new(AtomicUsize::new(0));
    let observed = commits.clone();
    conn.commit_hook(Some(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        false
    }))
    .unwrap();
    let id = init_db(&conn).unwrap();
    assert_eq!(commits.load(Ordering::SeqCst), 1);
    assert_eq!(conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), LIBRARY_SCHEMA_VERSION);
    drop(conn);
    let reopened = Connection::open(path).unwrap();
    assert_eq!(init_db(&reopened).unwrap(), id);
}

#[test]
fn late_initialization_error_rolls_back_schema_and_can_retry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let conn = Connection::open(&path).unwrap();
    conn.authorizer(Some(|context: AuthContext<'_>| if matches!(context.action, AuthAction::Pragma { pragma_name: "user_version", pragma_value: Some(_) }) { Authorization::Deny } else { Authorization::Allow })).unwrap();
    assert!(init_db(&conn).is_err());
    assert!(conn.is_autocommit());
    assert_eq!(conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), 0);
    let populated: bool = conn.query_row(include_str!("sql/init_db_select_3.sql"), [], |r| r.get(0)).unwrap();
    assert!(!populated, "no schema or seed tables survive the failed version stamp");
    drop(conn);
    let reopened = Connection::open(path).unwrap();
    let id = init_db(&reopened).unwrap();
    assert_eq!(init_db(&reopened).unwrap(), id);
}
