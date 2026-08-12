use std::path::PathBuf;
use std::time::{Duration, Instant};

use epub_svelte_lib::app::{AppBackend, AppDataLocation, BackendContext};
use epub_svelte_lib::platform::scanner::LibraryEvent;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let library_root = PathBuf::from(arguments.next().ok_or("usage: scan_benchmark LIBRARY_ROOT APP_DATA")?).canonicalize()?;
    let app_data = PathBuf::from(arguments.next().ok_or("usage: scan_benchmark LIBRARY_ROOT APP_DATA")?);
    if arguments.next().is_some() {
        return Err("usage: scan_benchmark LIBRARY_ROOT APP_DATA".into());
    }

    let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data))?;
    let existing = context.try_get_library_entries()?.into_iter().find(|entry| entry.library_path() == library_root).or_else(|| {
        let parent = library_root.parent()?;
        context.discover_libraries(parent).ok()?.into_iter().find(|entry| entry.library_path() == library_root)
    });
    let app = AppBackend::new(context)?;
    let events = app.take_events()?;
    let entry = match existing {
        Some(entry) => entry,
        None => app.create_library("Scanner benchmark".to_owned(), Some(library_root.clone()))?,
    };

    let started = Instant::now();
    let _library = app.library(entry.library_id())?;
    let mut thumbnail_events = 0_u64;
    let mut other_events = 0_u64;
    let mut scan_elapsed = None;
    let mut thumbnails_elapsed = None;
    loop {
        let app_event = events.recv_timeout(Duration::from_secs(30 * 60))?;
        if app_event.library_id != *entry.library_id() {
            continue;
        }
        match app_event.event {
            LibraryEvent::ScanComplete => {
                let elapsed = started.elapsed();
                println!("SCAN_INDEX_COMPLETE elapsed_ms={}", elapsed.as_millis());
                scan_elapsed = Some(elapsed);
            }
            LibraryEvent::ThumbnailGenerated { .. } => thumbnail_events += 1,
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, finished } => {
                thumbnail_events += 1;
                println!("SCAN_THUMBNAIL_BATCH available={} finished={} elapsed_ms={}", content_hashes.len(), finished, started.elapsed().as_millis());
                if finished {
                    thumbnails_elapsed = Some(started.elapsed());
                }
            }
            _ => other_events += 1,
        }
        if scan_elapsed.is_some() && thumbnails_elapsed.is_some() {
            break;
        }
    }

    println!(
        "SCAN_BENCHMARK root={} scan_ms={} thumbnails_ms={} thumbnail_events={} other_events={}",
        library_root.display(),
        scan_elapsed.expect("scan completed").as_millis(),
        thumbnails_elapsed.expect("thumbnails completed").as_millis(),
        thumbnail_events,
        other_events,
    );
    Ok(())
}
