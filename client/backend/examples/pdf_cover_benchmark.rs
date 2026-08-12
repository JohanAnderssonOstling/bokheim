use std::path::PathBuf;
use std::time::Instant;

use pdf_reader_core::RenderedPdfImageSource;
use walkdir::WalkDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args_os().nth(1).ok_or("usage: pdf_cover_benchmark LIBRARY_ROOT")?).canonicalize()?;
    let mut paths = WalkDir::new(&root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.path().extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")))
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    paths.sort();

    let started = Instant::now();
    let mut embedded = 0_usize;
    let mut rendered = 0_usize;
    let mut errors = 0_usize;
    for path in &paths {
        match pdf_reader_core::render_first_page_image(path, 300) {
            Ok(image) => match image.source() {
                RenderedPdfImageSource::EmbeddedThumbnail => {
                    embedded += 1;
                    println!("PDF_EMBEDDED_THUMBNAIL width={} height={} path={:?}", image.pixel_width(), image.pixel_height(), path);
                }
                RenderedPdfImageSource::RenderedPage => rendered += 1,
            },
            Err(_) => errors += 1,
        }
    }
    println!("PDF_COVER_BENCHMARK files={} embedded={} rendered={} errors={} wall_ms={}", paths.len(), embedded, rendered, errors, started.elapsed().as_millis());
    Ok(())
}
