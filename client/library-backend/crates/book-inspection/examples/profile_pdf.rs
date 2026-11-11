//! Read-only native PDF inspection profile: one path per input line, JSON output.
//! Hashing is outside inspection timings; phase and page diagnostics go to stderr.
use book_model::BookFormat;
use content_address::{BookId, ContentHash};
use serde_json::json;
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

struct PdfLogger;

impl log::Log for PdfLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "library_pdf_profile"
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}

#[derive(Default)]
struct IoTimings {
    read_ns: AtomicU64,
    seek_ns: AtomicU64,
    read_bytes: AtomicU64,
}

struct MeasuredReader {
    file: File,
    timings: Arc<IoTimings>,
}

impl Read for MeasuredReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let started = Instant::now();
        let result = self.file.read(bytes);
        self.timings.read_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if let Ok(count) = result {
            self.timings.read_bytes.fetch_add(count as u64, Ordering::Relaxed);
        }
        result
    }
}

impl Seek for MeasuredReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let started = Instant::now();
        let result = self.file.seek(position);
        self.timings.seek_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        result
    }
}

fn inspect(path: &Path) -> book_inspection::MetadataResult<serde_json::Value> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let book_id = BookId::from(ContentHash::from(hasher.finalize()));
    file.rewind()?;
    let timings = Arc::new(IoTimings::default());
    let mut reader = MeasuredReader { file, timings: timings.clone() };
    let identity_started = Instant::now();
    let (identity, declared) = book_inspection::prepare_pdf_metadata(path, &mut reader);
    let identity_ns = identity_started.elapsed().as_nanos();
    reader.rewind()?;
    let started = Instant::now();
    let book = book_inspection::inspect_book_reader_with_pdf_metadata(path, BookFormat::Pdf, reader, Some(book_id), None, Some(declared))?;
    let elapsed_ns = started.elapsed().as_nanos();
    Ok(
        json!({"path":path, "elapsed_ns":elapsed_ns, "identity_probe_ns":identity_ns, "source_identity":identity.map(|identity| identity.book_id.to_hex()), "read_ns":timings.read_ns.load(Ordering::Relaxed), "seek_ns":timings.seek_ns.load(Ordering::Relaxed), "read_bytes":timings.read_bytes.load(Ordering::Relaxed), "metadata":book.metadata, "toc":book.toc, "reader_metadata":book.pdf}),
    )
}

fn main() {
    static LOGGER: PdfLogger = PdfLogger;
    log::set_logger(&LOGGER).expect("initialize PDF inspection profile logger");
    log::set_max_level(log::LevelFilter::Debug);
    for line in io::stdin().lock().lines() {
        let path = line.expect("read PDF path");
        eprintln!("{}", json!({"event":"pdf_begin", "path":path}));
        match inspect(Path::new(&path)) {
            Ok(record) => println!("{record}"),
            Err(error) => println!("{}", json!({"path":path, "error":error.to_string()})),
        }
    }
}
