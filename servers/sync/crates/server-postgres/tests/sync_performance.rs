use server_postgres::{migrate, PostgresDatabase, PostgresSyncRepository};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Instant;
use sync_common::api::libraries::LibraryNameRequest;
use sync_common::{ConflictRank, LibraryId, OpaquePublication, OpaqueStateEntry, ReplicaId, ReplicaSeq, StateAddress, SyncCursor, SyncExchangeRequest, SyncExchangeResponse, VersionKey, Versioned, MAX_PUSH_BATCH_BYTES, MAX_PUSH_MUTATIONS};

/// A release-mode benchmark of the real PostgreSQL exchange and wire codec.
/// Every invocation owns its schema; results include no network/auth latency.
#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL; run in release mode with --nocapture"]
async fn postgres_sync_performance() {
    let url = std::env::var("SYNC_E2E_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("sync_performance_{}", LibraryId::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new().max_connections(4).after_connect(move |connection, _| {
        let statement = search_path.clone();
        Box::pin(async move { sqlx::query(&statement).execute(connection).await.map(|_| ()) })
    }).connect(&url).await.unwrap();
    let database = PostgresDatabase::from_pool(pool);
    migrate(&database).await.unwrap();
    sqlx::query("INSERT INTO users(id) VALUES('performance-user')").execute(database.pool()).await.unwrap();
    let repository = PostgresSyncRepository::new(database.clone());
    for (name, cells, payload_bytes) in [("small", 2_000, 128), ("large", 96, 128 * 1024)] {
        let library = LibraryId::new_v4();
        let replica = ReplicaId::new_v4();
        repository.create_library("performance-user", &LibraryNameRequest { library_id: library, library_name: name.into() }).await.unwrap();
        let batch = MAX_PUSH_MUTATIONS.min(MAX_PUSH_BATCH_BYTES / (payload_bytes + 1024));
        for start in (0..cells).step_by(batch) {
            let mutations = (start..(start + batch).min(cells)).map(|index| {
                let seq = ReplicaSeq::new(index as u64 + 1).unwrap();
                OpaquePublication {
                    publisher_seq: seq,
                    state: Versioned {
                        value: OpaqueStateEntry { address: StateAddress { kind: "directory_name".into(), entity_key: format!("cell-{index:05}").into(), entity_subkey: String::new() }, book: None, bytes: vec![7; payload_bytes] },
                        version: VersionKey::new(1, ConflictRank::Update, replica, seq),
                    },
                }
            }).collect();
            repository.exchange("performance-user", &SyncExchangeRequest { library_id: library, replica_id: replica, mutations, cursor: SyncCursor::default() }).await.unwrap();
        }
        let mut timings = Vec::new();
        let mut database_times = Vec::new();
        let mut encode_times = Vec::new();
        let mut decode_times = Vec::new();
        let mut page_count = 0;
        let mut maximum_page = 0;
        for _ in 0..10 {
            let pass = Instant::now();
            let mut db_ms = 0.0;
            let mut encode_ms = 0.0;
            let mut decode_ms = 0.0;
            let mut count = 0;
            let mut cursor = SyncCursor::default();
            let mut pages = 0;
            loop {
                let started = Instant::now();
                let response = repository.exchange("performance-user", &SyncExchangeRequest { library_id: library, replica_id: replica, mutations: Vec::new(), cursor }).await.unwrap();
                db_ms += started.elapsed().as_secs_f64() * 1_000.0;
                let started = Instant::now();
                let bytes = sync_common::transport::encode(&response).unwrap();
                encode_ms += started.elapsed().as_secs_f64() * 1_000.0;
                maximum_page = maximum_page.max(bytes.len());
                let started = Instant::now();
                let received: SyncExchangeResponse = sync_common::transport::decode_byte_fields(&bytes, sync_common::transport::MAX_DECODED_RESPONSE_BYTES).unwrap();
                decode_ms += started.elapsed().as_secs_f64() * 1_000.0;
                count += received.pull.mutations.len();
                cursor = received.pull.next_cursor;
                pages += 1;
                if !received.pull.has_more { break; }
            }
            assert_eq!(count, cells);
            page_count = pages;
            timings.push(pass.elapsed().as_secs_f64() * 1_000.0);
            database_times.push(db_ms);
            encode_times.push(encode_ms);
            decode_times.push(decode_ms);
        }
        for samples in [&mut timings, &mut database_times, &mut encode_times, &mut decode_times] { samples.sort_by(f64::total_cmp); }
        println!("PERFORMANCE case={name} cells={cells} payload_bytes={payload_bytes} pages={page_count} max_page_bytes={maximum_page} median_total_ms={:.3} median_database_ms={:.3} median_encode_ms={:.3} median_decode_ms={:.3}", timings[5], database_times[5], encode_times[5], decode_times[5]);
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(line) = status.lines().find(|line| line.starts_with("VmHWM:")) { println!("PERFORMANCE {line}"); }
    }
    database.pool().close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE")).execute(&admin).await.unwrap();
    admin.close().await;
}
