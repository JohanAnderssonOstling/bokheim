//! Profile native folder registration, scanning, thumbnails, and query latency.
//! Use a new app-data directory and a disposable snapshot of the book folder.
//! RUST_LOG=library_add_profile=debug enables scan-stage timings on stderr.
//! The `scan` completion mode parks thumbnail CPU requests before they acquire
//! shared inspection permits, so thumbnail work does not distort scan timings.

use app::{AppClient, AppDataLocation, host::BackendLaunch};
use client_platform_runtime::executor::{BackendExecutor, BoxedBackendFuture};
use cpu_host::{BookTask, BoxedCpuReader, CpuHost, CpuRequest, CpuResponse, NativeHost};
use library_database::rusqlite::{Connection, OpenFlags};
use library_management::account::{AccountSession, SessionStorage};
use library_management::management::{LibraryService, LibraryServiceConfig, ManagementClient};
use library_model::{BrowseLocation, LibraryBrowseQuery};
use library_registry::LibraryRegistry;
use serde_json::json;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct ScanHost;

impl CpuHost for ScanHost {
    fn book(&self, task: BookTask, reader: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<CpuResponse, app::BackendError>> {
        if matches!(task, BookTask::Thumbnail { .. }) {
            Box::pin(std::future::pending())
        } else {
            NativeHost.book(task, reader)
        }
    }

    fn request(&self, request: CpuRequest) -> BoxedBackendFuture<'_, Result<CpuResponse, app::BackendError>> {
        if matches!(request, CpuRequest::Cover { .. }) {
            Box::pin(std::future::pending())
        } else {
            NativeHost.request(request)
        }
    }
}

fn scan_management(data: &std::path::Path) -> Result<ManagementClient, Box<dyn Error>> {
    let executor = BackendExecutor::new()?;
    let registry = LibraryRegistry::open(data.to_string_lossy().into_owned()).map_err(|error| error.to_string())?;
    let account = AccountSession::load(SessionStorage::new(registry.clone()), app::api_server_url())?;
    let preferences = data.join("preferences");
    fs::create_dir_all(&preferences)?;
    let service = LibraryService::new(LibraryServiceConfig {
        registry,
        preference_storage_root: preferences,
        executor: executor.clone(),
        cpu: Arc::new(ScanHost),
        account: account.library_account(),
        notification_interest: Default::default(),
        sync_server: app::api_server_url(),
        metadata_server: app::METADATA_SERVER_ORIGIN.to_owned(),
        library_list_changed: tokio::sync::watch::channel(()).0,
        account_wake: tokio::sync::watch::channel(()).0,
        cloud_quota_lock: Arc::default(),
        library_location_lock: Arc::default(),
        storage_copy_lock: Arc::default(),
        root_failures: async_channel::unbounded().0,
    });
    library_management::start_catalog(data, &executor, &service);
    Ok(ManagementClient::local(service, Arc::new(())))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let data = PathBuf::from(arguments.next().ok_or("usage: library_add_profile NEW_APP_DATA BOOK_FOLDER [TIMEOUT_SECONDS]")?);
    let folder = PathBuf::from(arguments.next().ok_or("provide a disposable book-folder snapshot")?).canonicalize()?;
    let timeout: u64 = arguments.next().map(|value| value.to_string_lossy().parse()).transpose()?.unwrap_or(900);
    let scan_only = match arguments.next().as_deref() {
        None => false,
        Some(value) if value == "scan" => true,
        Some(_) => return Err("optional completion mode must be 'scan'".into()),
    };
    fs::create_dir(&data)?;
    fs::write(data.join("profile-isolated"), b"library-add-profile\n")?;
    env_logger::Builder::from_default_env().format(|buffer, record| writeln!(buffer, "[{:?} {} {}] {}", std::thread::current().id(), record.level(), record.target(), record.args())).init();
    let startup = Instant::now();
    let (management, _app) = if scan_only {
        (scan_management(&data)?, None)
    } else {
        let launch = BackendLaunch::initialize(AppDataLocation::native_path(&data))?;
        let (client, _) = AppClient::start_native(launch)?;
        (client.management.clone(), Some(client))
    };
    println!("{}", json!({"event": "startup", "elapsed_ms": startup.elapsed().as_secs_f64() * 1000.0, "thumbnail_cpu_paused":scan_only}));
    let executor = management.executor().clone();
    executor.block_on(async {
        let started = Instant::now();
        let (ids, errors) = management.create_libraries_with_asset_storage(vec![folder.to_string_lossy().into_owned()], false).await?;
        if !errors.is_empty() {
            return Err(format!("folder registration failed: {errors:?}").into());
        }
        let id = *ids.first().ok_or("no library was registered")?;
        println!("{}", json!({"event": "registered", "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0, "library_id": id}));
        let library = management.library(id)?;
        let (updates, _) = library.updates().await?;
        let monitor = Connection::open_with_flags(data.join("libraries").join(id.to_string()).join("library.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        monitor.busy_timeout(Duration::from_millis(100))?;
        let mut first_books = None;
        let mut scan_completed = None;
        let mut previous_count = 0;
        let mut settled = None;
        let mut covers = 0;
        let mut failures = 0;
        loop {
            while let Ok(update) = updates.try_recv() {
                match update {
                    library_model::LibraryUpdate::Scanning(true) => {
                        // Short follow-up scans can begin and finish between
                        // samples; their events still reset the quiet period.
                        settled = None;
                        scan_completed = None;
                    }
                    library_model::LibraryUpdate::Covers(books) => covers += books.len(),
                    library_model::LibraryUpdate::ThumbnailFailed { .. } => failures += 1,
                    _ => {}
                }
            }
            let queried = Instant::now();
            let count = library.book_count().await?;
            let count_ms = queried.elapsed().as_secs_f64() * 1000.0;
            let scanning = library.scanning().await?;
            let (pending, ready, no_cover): (i64, i64, i64) = monitor.query_row(
                "SELECT COALESCE(SUM(cover_state='pending'),0),COALESCE(SUM(cover_state='ready'),0),COALESCE(SUM(cover_state='no_cover'),0) FROM book_transfer",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            if scanning {
                scan_completed = None;
            }
            if count > 0 {
                first_books.get_or_insert(started.elapsed().as_secs_f64());
                if !scanning {
                    scan_completed.get_or_insert(started.elapsed().as_secs_f64());
                }
            }
            let query = LibraryBrowseQuery { location: BrowseLocation::Books, search: String::new(), file_types: vec![], chip_sort: Default::default(), book_sort: Default::default(), languages: vec![], hide_finished: false };
            let queried = Instant::now();
            library.browse_section_books(query, 0, 24).await?;
            let browse_ms = queried.elapsed().as_secs_f64() * 1000.0;
            println!("{}", json!({"event": "sample", "elapsed_s": started.elapsed().as_secs_f64(), "books": count, "scanning": scanning, "thumbnails_pending": pending, "thumbnails_ready": ready, "thumbnails_no_cover": no_cover, "cover_updates": covers, "thumbnail_failures": failures, "count_ms": count_ms, "browse_24_ms": browse_ms}));
            if count > 0 && !scanning && (scan_only || pending == 0) && count == previous_count {
                let since = settled.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_secs(5) {
                    println!("{}", json!({"event": "finished", "elapsed_s": started.elapsed().as_secs_f64(), "first_books_s": first_books, "scan_completed_s": scan_completed, "books": count, "thumbnails_ready": ready, "thumbnails_no_cover": no_cover, "thumbnail_failures": failures}));
                    return Ok::<_, Box<dyn Error>>(());
                }
            } else {
                settled = None;
            }
            previous_count = count;
            if started.elapsed() >= Duration::from_secs(timeout) {
                return Err("profiling deadline reached while work remains".into());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}
