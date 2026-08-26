use std::collections::BTreeMap;
use std::hint::black_box;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use library_model::BookFormat;
use walkdir::WalkDir;

#[derive(Clone)]
struct BookFile {
    path: PathBuf,
    format: BookFormat,
}

#[derive(Clone, Copy)]
enum Outcome {
    Produced,
    NoOutput,
    Error,
    Panic,
    Timeout,
}

struct Sample {
    path: PathBuf,
    format: BookFormat,
    elapsed: Duration,
    outcome: Outcome,
    bytes: u64,
}

#[derive(Default)]
struct Summary {
    samples: Vec<Sample>,
    wall: Duration,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let first = arguments.next().ok_or("usage: scan_component_benchmark LIBRARY_ROOT")?;
    if first == "--thumbnail-child" {
        let path = PathBuf::from(arguments.next().ok_or("missing thumbnail path")?);
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| thumbnail::generate_thumbnail(&path)));
        let exit_code = match result {
            Ok(Ok(Some(_))) => 0,
            Ok(Ok(None)) => 10,
            Ok(Err(_)) => 11,
            Err(_) => 12,
        };
        std::process::exit(exit_code);
    }
    let root = PathBuf::from(first).canonicalize()?;
    let mut isolate_thumbnails = false;
    let mut thumbnail_worker_override = None;
    for argument in arguments {
        let argument = argument.to_str().ok_or("benchmark arguments must be UTF-8")?;
        if argument == "--isolate-thumbnails" {
            isolate_thumbnails = true;
        } else if let Some(value) = argument.strip_prefix("--thumbnail-workers=") {
            thumbnail_worker_override = Some(value.parse::<usize>()?.clamp(1, 32));
        } else {
            return Err("usage: scan_component_benchmark LIBRARY_ROOT [--isolate-thumbnails] [--thumbnail-workers=N]".into());
        }
    }
    let worker_count = std::thread::available_parallelism().map(|count| count.get()).unwrap_or(2).clamp(1, 4);

    let enumerate_started = Instant::now();
    let mut books = WalkDir::new(&root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| format_for_path(entry.path()).map(|format| BookFile { path: entry.into_path(), format }))
        .collect::<Vec<_>>();
    books.sort_by(|left, right| left.path.cmp(&right.path));
    let enumerate_elapsed = enumerate_started.elapsed();
    println!("COMPONENT phase=enumerate files={} wall_ms={}", books.len(), enumerate_elapsed.as_millis());

    let fingerprint_started = Instant::now();
    let mut fingerprint_failures = 0_usize;
    let mut total_bytes = 0_u64;
    for book in &books {
        match std::fs::metadata(&book.path) {
            Ok(metadata) => {
                total_bytes += metadata.len();
                black_box((metadata.len(), metadata.modified()?));
            }
            Err(_) => fingerprint_failures += 1,
        }
    }
    println!("COMPONENT phase=fingerprint files={} bytes={} errors={} wall_ms={}", books.len(), total_bytes, fingerprint_failures, fingerprint_started.elapsed().as_millis());

    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let hash = benchmark_hashing(&books, worker_count);
    print_summary("hash", worker_count, &hash);

    let metadata = benchmark_metadata(&books, worker_count);
    print_summary("metadata", worker_count, &metadata);

    let thumbnail_workers = if isolate_thumbnails { 1 } else { thumbnail_worker_override.unwrap_or(worker_count) };
    let thumbnails = benchmark_thumbnails(&books, isolate_thumbnails, thumbnail_workers);
    print_summary("thumbnail", thumbnail_workers, &thumbnails);

    std::panic::set_hook(old_hook);
    Ok(())
}

fn format_for_path(path: &Path) -> Option<BookFormat> {
    path.extension().and_then(|extension| extension.to_str()).and_then(BookFormat::from_import_extension)
}

fn benchmark_hashing(books: &[BookFile], worker_count: usize) -> Summary {
    let (task_tx, task_rx) = crossbeam_channel::bounded::<BookFile>(256);
    let (result_tx, result_rx) = crossbeam_channel::unbounded::<Sample>();
    let completed = AtomicUsize::new(0);
    let wall_started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let task_rx = task_rx.clone();
            let result_tx = result_tx.clone();
            let completed = &completed;
            scope.spawn(move || {
                let mut buffer = vec![0_u8; 1024 * 1024];
                while let Ok(book) = task_rx.recv() {
                    let started = Instant::now();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut file = std::fs::File::open(&book.path)?;
                        let mut hasher = blake3::Hasher::new();
                        let mut bytes = 0_u64;
                        loop {
                            let read = file.read(&mut buffer)?;
                            if read == 0 {
                                break;
                            }
                            hasher.update(&buffer[..read]);
                            bytes += read as u64;
                        }
                        black_box(hasher.finalize());
                        Ok::<_, std::io::Error>(bytes)
                    }));
                    let (outcome, bytes) = match result {
                        Ok(Ok(bytes)) => (Outcome::Produced, bytes),
                        Ok(Err(_)) => (Outcome::Error, 0),
                        Err(_) => (Outcome::Panic, 0),
                    };
                    let _ = result_tx.send(Sample { path: book.path, format: book.format, elapsed: started.elapsed(), outcome, bytes });
                    progress("hash", completed.fetch_add(1, Ordering::Relaxed) + 1);
                }
            });
        }
        for book in books.iter().cloned() {
            task_tx.send(book).expect("hash workers stopped");
        }
        drop(task_tx);
        drop(result_tx);
    });
    Summary { samples: result_rx.iter().collect(), wall: wall_started.elapsed() }
}

fn benchmark_metadata(books: &[BookFile], worker_count: usize) -> Summary {
    let (task_tx, task_rx) = crossbeam_channel::bounded::<BookFile>(256);
    let (result_tx, result_rx) = crossbeam_channel::unbounded::<Sample>();
    let completed = AtomicUsize::new(0);
    let wall_started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let task_rx = task_rx.clone();
            let result_tx = result_tx.clone();
            let completed = &completed;
            scope.spawn(move || {
                while let Ok(book) = task_rx.recv() {
                    let started = Instant::now();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| book_metadata::inspect_book(&book.path, book.format)));
                    let outcome = match result {
                        Ok(Ok(_)) => Outcome::Produced,
                        Ok(Err(_)) => Outcome::Error,
                        Err(_) => Outcome::Panic,
                    };
                    let _ = result_tx.send(Sample { path: book.path, format: book.format, elapsed: started.elapsed(), outcome, bytes: 0 });
                    progress("metadata", completed.fetch_add(1, Ordering::Relaxed) + 1);
                }
            });
        }
        for book in books.iter().cloned() {
            task_tx.send(book).expect("metadata workers stopped");
        }
        drop(task_tx);
        drop(result_tx);
    });
    Summary { samples: result_rx.iter().collect(), wall: wall_started.elapsed() }
}

fn benchmark_thumbnails(books: &[BookFile], isolate: bool, worker_count: usize) -> Summary {
    if isolate {
        return benchmark_thumbnails_isolated(books);
    }
    let image_worker_count = worker_count.saturating_sub(1).max(1);
    let (pdf_task_tx, pdf_task_rx) = crossbeam_channel::bounded::<BookFile>(256);
    let (image_task_tx, image_task_rx) = crossbeam_channel::bounded::<BookFile>(256);
    let (result_tx, result_rx) = crossbeam_channel::unbounded::<Sample>();
    let completed = AtomicUsize::new(0);
    let wall_started = Instant::now();
    std::thread::scope(|scope| {
        {
            let result_tx = result_tx.clone();
            let completed = &completed;
            scope.spawn(move || run_thumbnail_benchmark_worker(pdf_task_rx, result_tx, completed));
        }
        for _ in 0..image_worker_count {
            let task_rx = image_task_rx.clone();
            let result_tx = result_tx.clone();
            let completed = &completed;
            scope.spawn(move || run_thumbnail_benchmark_worker(task_rx, result_tx, completed));
        }
        for book in books.iter().cloned() {
            if book.format == BookFormat::Pdf {
                pdf_task_tx.send(book).expect("PDF thumbnail worker stopped");
            } else {
                image_task_tx.send(book).expect("image thumbnail workers stopped");
            }
        }
        drop(pdf_task_tx);
        drop(image_task_tx);
        drop(result_tx);
    });
    Summary { samples: result_rx.iter().collect(), wall: wall_started.elapsed() }
}

fn run_thumbnail_benchmark_worker(task_rx: crossbeam_channel::Receiver<BookFile>, result_tx: crossbeam_channel::Sender<Sample>, completed: &AtomicUsize) {
    while let Ok(book) = task_rx.recv() {
        let sample = generate_thumbnail_sample(book);
        let _ = result_tx.send(sample);
        progress("thumbnail", completed.fetch_add(1, Ordering::Relaxed) + 1);
    }
}

fn generate_thumbnail_sample(book: BookFile) -> Sample {
    let started = Instant::now();
    let (outcome, bytes) = if !matches!(book.format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b) {
        (Outcome::NoOutput, 0)
    } else {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| thumbnail::generate_thumbnail(&book.path))) {
            Ok(Ok(Some(thumbnail))) => (Outcome::Produced, thumbnail.len() as u64),
            Ok(Ok(None)) => (Outcome::NoOutput, 0),
            Ok(Err(_)) => (Outcome::Error, 0),
            Err(_) => (Outcome::Panic, 0),
        }
    };
    Sample { path: book.path, format: book.format, elapsed: started.elapsed(), outcome, bytes }
}

fn benchmark_thumbnails_isolated(books: &[BookFile]) -> Summary {
    const FILE_TIMEOUT: Duration = Duration::from_secs(15);
    let wall_started = Instant::now();
    let mut samples = Vec::with_capacity(books.len());
    let executable = std::env::current_exe().expect("benchmark executable path");
    for (index, book) in books.iter().enumerate() {
        let started = Instant::now();
        let outcome = if !matches!(book.format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b) {
            (Outcome::NoOutput, 0)
        } else {
            let outcome = match Command::new(&executable).arg("--thumbnail-child").arg(&book.path).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                Ok(mut child) => loop {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            break match status.code() {
                                Some(0) => Outcome::Produced,
                                Some(10) => Outcome::NoOutput,
                                Some(12) => Outcome::Panic,
                                _ => Outcome::Error,
                            };
                        }
                        Ok(None) if started.elapsed() < FILE_TIMEOUT => std::thread::sleep(Duration::from_millis(10)),
                        Ok(None) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break Outcome::Timeout;
                        }
                        Err(_) => break Outcome::Error,
                    }
                },
                Err(_) => Outcome::Error,
            };
            (outcome, 0)
        };
        samples.push(Sample { path: book.path.clone(), format: book.format, elapsed: started.elapsed(), outcome: outcome.0, bytes: outcome.1 });
        progress("thumbnail", index + 1);
    }
    Summary { samples, wall: wall_started.elapsed() }
}

fn progress(phase: &str, completed: usize) {
    if completed % 25 == 0 {
        eprintln!("PROGRESS phase={phase} completed={completed}");
    }
}

fn print_summary(phase: &str, workers: usize, summary: &Summary) {
    println!("COMPONENT phase={phase} workers={workers} wall_ms={}", summary.wall.as_millis());
    let mut by_format = BTreeMap::<&'static str, Vec<&Sample>>::new();
    for sample in &summary.samples {
        by_format.entry(format_name(sample.format)).or_default().push(sample);
    }
    for (format, samples) in by_format {
        let mut elapsed = samples.iter().map(|sample| sample.elapsed).collect::<Vec<_>>();
        elapsed.sort_unstable();
        let produced = samples.iter().filter(|sample| matches!(sample.outcome, Outcome::Produced)).count();
        let no_output = samples.iter().filter(|sample| matches!(sample.outcome, Outcome::NoOutput)).count();
        let errors = samples.iter().filter(|sample| matches!(sample.outcome, Outcome::Error)).count();
        let panics = samples.iter().filter(|sample| matches!(sample.outcome, Outcome::Panic)).count();
        let timeouts = samples.iter().filter(|sample| matches!(sample.outcome, Outcome::Timeout)).count();
        let bytes = samples.iter().map(|sample| sample.bytes).sum::<u64>();
        let cpu = elapsed.iter().copied().sum::<Duration>();
        println!(
            "FORMAT phase={phase} format={format} files={} produced={produced} no_output={no_output} errors={errors} panics={panics} timeouts={timeouts} bytes={bytes} work_ms={} p50_ms={} p95_ms={} max_ms={}",
            samples.len(),
            cpu.as_millis(),
            percentile(&elapsed, 50).as_millis(),
            percentile(&elapsed, 95).as_millis(),
            elapsed.last().copied().unwrap_or_default().as_millis(),
        );
    }
    let mut slowest = summary.samples.iter().collect::<Vec<_>>();
    slowest.sort_unstable_by_key(|sample| std::cmp::Reverse(sample.elapsed));
    for sample in slowest.into_iter().take(5) {
        println!("SLOW phase={phase} format={} elapsed_ms={} path={:?}", format_name(sample.format), sample.elapsed.as_millis(), sample.path);
    }
    for sample in summary.samples.iter().filter(|sample| matches!(sample.outcome, Outcome::Timeout)) {
        println!("TIMEOUT phase={phase} format={} elapsed_ms={} path={:?}", format_name(sample.format), sample.elapsed.as_millis(), sample.path);
    }
}

fn percentile(sorted: &[Duration], percentile: usize) -> Duration {
    sorted.get(sorted.len().saturating_sub(1) * percentile / 100).copied().unwrap_or_default()
}

fn format_name(format: BookFormat) -> &'static str {
    match format {
        BookFormat::Epub => "epub",
        BookFormat::Pdf => "pdf",
        BookFormat::Mobi => "mobi",
        BookFormat::Fb2 => "fb2",
        BookFormat::Cbz => "cbz",
        BookFormat::Html => "html",
        BookFormat::WebPub => "webpub",
        BookFormat::M4b => "m4b",
    }
}
