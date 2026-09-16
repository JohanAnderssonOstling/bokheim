//! Test-only real OPFS database adapter for the browser sync boundary contract.
use crate::storage::database::Database;
use std::cell::RefCell;
use std::sync::atomic::AtomicBool;

thread_local! {
    static DATABASE: RefCell<Option<Database>> = const { RefCell::new(None) };
    static WAKE_HINTS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    static REMOTE_PAGES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub async fn command(action: &str, server: &str) -> Result<String, String> {
    if action == "initialize" {
        crate::platform::web_storage::initialize().await?;
        let database = Database::open_library("sync-worker-contract", client_library::schema::configure_connection).map_err(string)?;
        client_library::schema::init_db(&*database.get().map_err(string)?).map_err(string)?;
        {
            let mut conn = database.get().map_err(string)?;
            let exists: bool = conn.query_row(include_str!("sql/sync_contract/command_select.sql"), ["a".repeat(64)], |row| row.get(0)).map_err(string)?;
            if !exists {
                let tx = conn.transaction().map_err(string)?;
                tx.execute(include_str!("sql/sync_contract/command_insert.sql"), ["a".repeat(64)]).map_err(string)?;
                // Begin with an established book and no pending setup mutations.
                tx.execute(include_str!("sql/sync_contract/command_delete.sql"), []).map_err(string)?;
                tx.commit().map_err(string)?;
            }
        }
        REMOTE_PAGES.with(|count| count.set(0));
        WAKE_HINTS.with(|flags| flags.set(0));
        database.observe_writes(|writes| {
            WAKE_HINTS.with(|flags| {
                flags.set(flags.get() | u8::from(writes.metadata) | (u8::from(writes.assets) << 1));
            })
        });
        database.observe_remote_pages(|| REMOTE_PAGES.with(|count| count.set(count.get() + 1)));
        DATABASE.with(|slot| *slot.borrow_mut() = Some(database));
        return Ok("ready".into());
    }
    let database = DATABASE.with(|slot| slot.borrow().clone()).ok_or("initialize first")?;
    if action == "wake_hints" {
        return Ok(WAKE_HINTS.with(|flags| flags.replace(0)).to_string());
    }
    if action == "remote_pages" {
        return Ok(REMOTE_PAGES.with(|count| count.get()).to_string());
    }
    if action == "asset_work" {
        return database.get().map_err(string)?.query_row(include_str!("sql/sync_contract/command_select_2.sql"), [], |row| row.get::<_, i64>(0)).map(|count| count.to_string()).map_err(string);
    }
    if action == "clear_asset_work" {
        database.get().map_err(string)?.execute(include_str!("sql/sync_contract/command_delete_2.sql"), []).map_err(string)?;
        return Ok("cleared".into());
    }
    if action == "received_title" {
        return database.get().map_err(string)?.query_row(include_str!("sql/sync_contract/command_select_3.sql"), ["c".repeat(64)], |row| row.get(0)).map_err(string);
    }
    if action == "old_schema_rejection" {
        let old = Database::open_library("unsupported-schema-contract", client_library::schema::configure_connection).map_err(string)?;
        let conn = old.get().map_err(string)?;
        conn.execute_batch(include_str!("sql/sync_contract/command_create.sql")).map_err(string)?;
        let error = client_library::schema::init_db(&conn).expect_err("older schemas must be rejected").to_string();
        assert!(error.contains("unsupported library schema version 29"), "{error}");
        assert_eq!(conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).map_err(string)?, 29);
        assert_eq!(conn.query_row(include_str!("sql/sync_contract/command_select_4.sql"), [], |row| row.get::<_, String>(0)).map_err(string)?, "keep");
        assert_eq!(conn.query_row(include_str!("sql/sync_contract/command_select_5.sql"), [], |row| row.get::<_, i64>(0)).map_err(string)?, 1);
        return Ok(error);
    }
    if action == "probe" {
        database
            .command(crate::storage::storage_queue::Priority::Interactive, |conn| {
                conn.query_row(include_str!("sql/sync_contract/command_select_6.sql"), [], |row| row.get::<_, i64>(0))?;
                Ok(())
            })
            .await
            .map_err(string)?;
        return Ok("responsive".into());
    }
    if action == "change_sync_account" {
        client_library::sync_state::rebind_sync_account(&*database.get().map_err(string)?, "sync-replacement-user").map_err(string)?;
        return Ok("changed".into());
    }
    if matches!(action, "seed_download" | "download" | "download_present" | "evict_download" | "change_download_account" | "download_pending") {
        let bytes = include_bytes!("../../tests/fixtures/storage-contract.epub");
        let hash = crate::ContentHash::new(blake3::hash(bytes).to_hex().as_str());
        let assets = crate::storage::files::open("sync-worker-contract").map_err(string)?;
        if action == "change_download_account" {
            client_library::sync_state::rebind_sync_account(&*database.get().map_err(string)?, "replacement-user").map_err(string)?;
            client_library::assets::request_book_downloads(&mut *database.get().map_err(string)?, &[hash], crate::TransferOrigin::UserInitiated).map_err(string)?;
            return Ok("changed".into());
        }
        if action == "download_pending" {
            return Ok(client_library::assets::get_asset_requests(&*database.get().map_err(string)?, crate::BlobKind::Book).map_err(string)?.iter().any(|(pending, _)| pending == &hash).to_string());
        }
        if action == "seed_download" {
            let mut conn = database.get().map_err(string)?;
            let tx = conn.transaction().map_err(string)?;
            tx.execute(include_str!("sql/sync_contract/command_insert_2.sql"), [hash.as_str()]).map_err(string)?;
            tx.execute(include_str!("sql/sync_contract/command_insert_3.sql"), [hash.as_str()]).map_err(string)?;
            client_library::sync_state::set_sync_metadata(&tx, "bound_user_id", "contract-user").map_err(string)?;
            tx.commit().map_err(string)?;
            client_library::assets::request_book_downloads(&mut conn, &[hash], crate::TransferOrigin::UserInitiated).map_err(string)?;
            return Ok(hash.to_string());
        }
        if action == "evict_download" {
            assets.remove(crate::BlobKind::Book, &hash).map_err(string)?;
            client_library::assets::request_book_downloads(&mut *database.get().map_err(string)?, &[hash], crate::TransferOrigin::UserInitiated).map_err(string)?;
            return Ok("evicted".into());
        }
        if action == "download_present" {
            return Ok(assets.exists(crate::BlobKind::Book, &hash).map_err(string)?.to_string());
        }
        let server_url = crate::ServerUrl::parse(server).map_err(string)?;
        let credentials = std::sync::Arc::new(std::sync::RwLock::new(Some(sync_transport::SyncCredentials::new(server_url.clone(), "contract-token"))));
        let sync = crate::sync::LibrarySync::new(crate::sync::LibrarySyncConfig {
            account: None,
            notification_interest: Default::default(),
            database,
            assets: assets.clone(),
            server_url,
            credentials,
            library_id: crate::LibraryId::parse_str("00000000-0000-4000-8000-000000000001").map_err(string)?,
            replica_id: sync_common::ReplicaId::parse_str("00000000-0000-4000-8000-000000000002").map_err(string)?,
            event_tx: None,
            transfer_history_path: "unused-contract-history".into(),
        });
        sync.download_requested(hash).await.map_err(string)?;
        return Ok(if assets.exists(crate::BlobKind::Book, &hash).map_err(string)? { "downloaded" } else { "missing" }.into());
    }
    if matches!(action, "sync" | "sync_inventory" | "repair") {
        let credentials = sync_transport::SyncCredentials::new(sync_transport::ServerUrl::parse(server).map_err(string)?, "contract-token");
        crate::worker_sync::synchronize(
            database,
            sync_common::LibraryId::parse_str("00000000-0000-4000-8000-000000000001").map_err(string)?,
            sync_common::ReplicaId::parse_str("00000000-0000-4000-8000-000000000002").map_err(string)?,
            "contract-user",
            &credentials,
            &AtomicBool::new(action == "sync"),
            false,
            action != "sync",
        )
        .await
        .map_err(string)?;
        return Ok("synced".into());
    }
    if action == "seed_recovery" {
        let mut conn = database.get().map_err(string)?;
        let tx = conn.transaction().map_err(string)?;
        for id in 1..=300 {
            tx.execute(include_str!("sql/sync_contract/command_insert_4.sql"), [format!("{id:064x}")]).map_err(string)?;
        }
        tx.commit().map_err(string)?;
        // Recovery must reconstruct all registers through bounded commands.
        conn.execute(include_str!("sql/sync_contract/command_delete.sql"), []).map_err(string)?;
        client_library::sync_state::begin_cursor_recovery(&conn).map_err(string)?;
        return Ok(client_library::sync_state::state_inventory(&conn).map_err(string)?.len().to_string());
    }
    if action == "edit" {
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        let conn = database.get().map_err(string)?;
        let current = client_library::reading::get_stored_reading_position(&conn, hash).map_err(string)?;
        let next = if current.as_ref().is_some_and(|position| position.as_str() == "epubcfi(/6/2)") { "epubcfi(/6/4)" } else { "epubcfi(/6/2)" };
        client_library::reading::update_reading_position(&conn, hash, next, None).map_err(string)?;
        return conn.query_row(include_str!("sql/sync_contract/command_select_7.sql"), [hash.as_str()], |row| row.get(0)).map_err(string);
    }
    if action == "pending" {
        return serde_json::to_string(&client_library::sync_state::get_pending_mutations(&*database.get().map_err(string)?).map_err(string)?.into_iter().map(|mutation| mutation.mutation_id.to_string()).collect::<Vec<_>>()).map_err(string);
    }
    Err(format!("unknown action: {action}"))
}
fn string(error: impl std::fmt::Display) -> String {
    error.to_string()
}
