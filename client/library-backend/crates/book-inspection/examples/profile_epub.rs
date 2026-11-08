//! Read-only EPUB inspection profile: one input path per line, JSON output.
//! Section read/decompression and parsing timings are written to stderr.
use serde_json::json;
use std::io::{self, BufRead};
use std::path::Path;
use std::time::Instant;

struct SectionLogger;

impl log::Log for SectionLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "library_epub_profile"
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}

fn main() {
    static LOGGER: SectionLogger = SectionLogger;
    log::set_logger(&LOGGER).expect("initialize inspection profile logger");
    log::set_max_level(log::LevelFilter::Debug);
    for line in io::stdin().lock().lines() {
        let path = line.expect("read EPUB path");
        eprintln!("{}", json!({"event":"epub_begin", "path":path}));
        let started = Instant::now();
        match book_inspection::inspect_epub(Path::new(&path)) {
            Ok(book) => {
                let elapsed_ns = started.elapsed().as_nanos();
                // Keep the package-only baseline outside the inspection timer.
                let declared = book_inspection::open_epub(Path::new(&path)).and_then(|mut provider| book_inspection::metadata_from_book(Path::new(&path), &mut provider)).ok();
                println!(
                    "{}",
                    json!({"path":path, "elapsed_ns":elapsed_ns, "container_ms":book.timings.container_ms, "evidence_ms":book.timings.evidence_ms, "toc_ms":book.timings.toc_ms, "declared_metadata":declared, "metadata":book.metadata, "toc":book.toc})
                );
            }
            Err(error) => println!("{}", json!({"path":path, "error":error.to_string()})),
        }
    }
}
