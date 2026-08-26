use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use library_model::BookFormat;
use walkdir::WalkDir;

fn supported(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).and_then(BookFormat::from_import_extension).is_some()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args_os().nth(1).ok_or("usage: hash_tree_benchmark LIBRARY_ROOT")?).canonicalize()?;
    let paths = WalkDir::new(&root).follow_links(false).min_depth(1).into_iter().filter_map(Result::ok).filter(|entry| entry.file_type().is_file() && supported(entry.path())).map(|entry| entry.into_path()).collect::<Vec<_>>();
    let expected_bytes = paths.iter().try_fold(0_u64, |total, path| std::fs::metadata(path).map(|metadata| total + metadata.len()))?;
    let worker_count = std::thread::available_parallelism().map(|count| count.get()).unwrap_or(2).clamp(1, 4);
    let (sender, receiver) = crossbeam_channel::bounded::<PathBuf>(256);
    let bytes = AtomicU64::new(0);
    let files = AtomicU64::new(0);
    let failures = AtomicU64::new(0);
    let started = Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let receiver = receiver.clone();
            let bytes = &bytes;
            let files = &files;
            let failures = &failures;
            scope.spawn(move || {
                while let Ok(path) = receiver.recv() {
                    let result = (|| {
                        let mut file = std::fs::File::open(&path)?;
                        let mut hasher = blake3::Hasher::new();
                        let copied = std::io::copy(&mut file, &mut hasher)?;
                        black_box(hasher.finalize());
                        Ok::<_, std::io::Error>(copied)
                    })();
                    match result {
                        Ok(copied) => {
                            bytes.fetch_add(copied, Ordering::Relaxed);
                            files.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            failures.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
        for path in paths {
            sender.send(path).expect("hash workers stopped");
        }
        drop(sender);
    });

    let elapsed = started.elapsed();
    let hashed_bytes = bytes.load(Ordering::Relaxed);
    println!(
        "HASH_TREE_BENCHMARK root={} workers={} files={} failures={} expected_bytes={} hashed_bytes={} elapsed_ms={} mib_per_second={:.1}",
        root.display(),
        worker_count,
        files.load(Ordering::Relaxed),
        failures.load(Ordering::Relaxed),
        expected_bytes,
        hashed_bytes,
        elapsed.as_millis(),
        hashed_bytes as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64(),
    );
    Ok(())
}
