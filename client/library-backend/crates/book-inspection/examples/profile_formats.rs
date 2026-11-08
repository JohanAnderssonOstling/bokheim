//! Read-only native MOBI/MP3 ingestion profile: paths on stdin, JSON on stdout.
use book_inspection::{InspectedBook, MetadataResult};
use content_address::BookId;
use serde_json::{json, Map, Value};
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

struct ProfileLogger;

impl log::Log for ProfileLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "library_mobi_profile"
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

struct Measured<R> {
    reader: R,
    timings: Arc<IoTimings>,
}

impl<R: Read> Read for Measured<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let started = Instant::now();
        let result = self.reader.read(bytes);
        self.timings.read_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if let Ok(count) = result {
            self.timings.read_bytes.fetch_add(count as u64, Ordering::Relaxed);
        }
        result
    }
}

impl<R: Seek> Seek for Measured<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let started = Instant::now();
        let result = self.reader.seek(position);
        self.timings.seek_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        result
    }
}

fn phase(started: Instant, timings: &IoTimings) -> Value {
    json!({"elapsed_ns":started.elapsed().as_nanos(), "read_ns":timings.read_ns.load(Ordering::Relaxed), "seek_ns":timings.seek_ns.load(Ordering::Relaxed), "read_bytes":timings.read_bytes.load(Ordering::Relaxed)})
}

fn mobi(path: &Path, stages: &mut Map<String, Value>) -> MetadataResult<(BookId, InspectedBook)> {
    let mut file = File::open(path)?;
    let timings = Arc::new(IoTimings::default());
    let started = Instant::now();
    let (hash, _) = content_address::hash_reader_with_size(&mut Measured { reader: &mut file, timings: timings.clone() })?;
    let id = BookId::from(hash);
    stages.insert("checksum".into(), phase(started, &timings));
    file.rewind()?;
    let timings = Arc::new(IoTimings::default());
    let started = Instant::now();
    let book = book_inspection::inspect_mobi_with_frontmatter(path, Measured { reader: file, timings: timings.clone() }, |book, text| book_inspection::inspect_mobi_frontmatter(book.clone(), text).ok().map(|book| book.metadata.book))?;
    stages.insert("inspection".into(), phase(started, &timings));
    Ok((id, book))
}

fn mp3(path: &Path, stages: &mut Map<String, Value>) -> MetadataResult<(BookId, InspectedBook)> {
    let started = Instant::now();
    let tracks = audiobook_folder::discover(path)?;
    stages.insert("discovery".into(), phase(started, &IoTimings::default()));
    let timings = Arc::new(IoTimings::default());
    let started = Instant::now();
    let content = audiobook_folder::folder_content_with_reader(path, &tracks, |path| Ok(Measured { reader: File::open(path)?, timings: timings.clone() }))?;
    stages.insert("content_hash".into(), phase(started, &timings));
    let id = BookId::from(content.checksum);
    let timings = Arc::new(IoTimings::default());
    let started = Instant::now();
    let book = book_inspection::inspect_audiobook_folder_content(path, path, id, &content.tracks, &tracks, |path| Ok(Measured { reader: File::open(path)?, timings: timings.clone() }))?;
    stages.insert("inspection".into(), phase(started, &timings));
    if std::env::var_os("BOKHEIM_PROFILE_TRANSPORT").is_some() {
        let started = Instant::now();
        let mut archive = audiobook_folder::folder_archive_reader(path, &tracks, |path| File::open(path))?;
        let size = archive.seek(SeekFrom::End(0))?;
        let archived = audiobook_archive::archived_tracks(&mut archive)?;
        let checksum = audiobook_archive::archive_content_hash(&mut archive)?;
        stages.insert(
            "transport_comparison".into(),
            json!({"elapsed_ns":started.elapsed().as_nanos(), "same_content":checksum == content.checksum, "same_tracks":archived == content.tracks, "same_length":size == content.archive_size, "archive_size":size}),
        );
    }
    Ok((id, book))
}

fn main() {
    static LOGGER: ProfileLogger = ProfileLogger;
    log::set_logger(&LOGGER).expect("initialize format inspection logger");
    log::set_max_level(log::LevelFilter::Debug);
    for line in io::stdin().lock().lines() {
        let line = line.expect("read book path");
        let path = Path::new(&line);
        eprintln!("{}", json!({"event":"book_begin", "path":path}));
        let mut stages = Map::new();
        let format = if path.is_dir() { "mp3folder" } else { "mobi" };
        let result = if path.is_dir() { mp3(path, &mut stages) } else { mobi(path, &mut stages) };
        match result {
            Ok((id, book)) => println!("{}", json!({"path":path, "format":format, "book_id":id.to_hex(), "stages":stages, "book":book})),
            Err(error) => println!("{}", json!({"path":path, "format":format, "stages":stages, "error":error.to_string()})),
        }
    }
}
