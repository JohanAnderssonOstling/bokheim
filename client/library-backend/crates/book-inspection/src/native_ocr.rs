//! Bounded native Tesseract fallback. This module and its dependencies are absent on WASM.
use crate::evidence::{self, SectionEvidence};
use book_model::{BookProperty, BookRecord, Scheme};
use epub_provider::EpubProvider;
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

const MAX_IMAGES: usize = 24;
const BOOK_BUDGET: Duration = Duration::from_secs(20);
const PROCESS_BUDGET: Duration = Duration::from_secs(5);
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
static OCR_ACTIVE: Mutex<usize> = Mutex::new(0);
static OCR_AVAILABLE: Condvar = Condvar::new();
struct OcrPermit;
impl OcrPermit {
    fn acquire() -> Option<Self> {
        let limit = book_enrichment::background_worker_limit(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
        let mut active = OCR_ACTIVE.lock().ok()?;
        while *active >= limit {
            active = OCR_AVAILABLE.wait(active).ok()?;
        }
        *active += 1;
        Some(Self)
    }
}
impl Drop for OcrPermit {
    fn drop(&mut self) {
        if let Ok(mut active) = OCR_ACTIVE.lock() {
            *active -= 1;
            OCR_AVAILABLE.notify_one();
        }
    }
}

pub(crate) fn needs_isbn(book: &BookRecord) -> bool {
    !book.book.identifiers.iter().any(|id| id.scheme() == &Scheme::Isbn && id.canonical_value().as_deref().and_then(book_enrichment::metadata_isbn).is_some()) && crate::related_isbn::lookup_candidates(&book.book.identifiers).is_empty()
}

pub(crate) struct Session {
    _guard: OcrPermit,
    directory: tempfile::TempDir,
    started: Instant,
    images: usize,
    data: Option<PathBuf>,
    status: &'static str,
}

impl Session {
    pub(crate) fn start(book: &BookRecord) -> Option<Self> {
        if !needs_isbn(book) {
            return None;
        }
        // Inspection callers already run outside the UI/database transaction.
        // Bound concurrent OCR by device capacity across all inspection callers.
        let guard = OcrPermit::acquire()?;
        let directory = tempfile::tempdir().ok()?;
        let data = std::env::var_os("TESSDATA_PREFIX").map(PathBuf::from).or_else(|| {
            let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))?;
            let path = base.join("bokheim/ocr/tessdata");
            path.join("eng.traineddata").is_file().then_some(path)
        });
        Some(Self { _guard: guard, directory, started: Instant::now(), images: 0, data, status: "complete" })
    }

    pub(crate) fn available(&mut self) -> bool {
        let output = self.run(&["--list-langs"]);
        if output.is_some_and(|text| text.lines().any(|line| line.trim() == "eng")) {
            return true;
        }
        self.status = "unavailable";
        false
    }

    pub(crate) fn can_continue(&mut self) -> bool {
        if self.images >= MAX_IMAGES || self.started.elapsed() >= BOOK_BUDGET {
            self.status = "budget-exhausted";
            return false;
        }
        self.status != "unavailable"
    }

    fn run(&mut self, args: &[&str]) -> Option<String> {
        // Opt-in diagnostics include failures and timeouts, without logging book text.
        struct Timing {
            started: Instant,
            enabled: bool,
            recognition: bool,
        }
        impl Drop for Timing {
            fn drop(&mut self) {
                if self.enabled {
                    eprintln!("bokheim_ocr_timing operation={} elapsed_ms={}", if self.recognition { "recognize" } else { "probe" }, self.started.elapsed().as_millis());
                }
            }
        }
        let _timing = Timing { started: Instant::now(), enabled: std::env::var_os("BOKHEIM_OCR_TIMING").is_some(), recognition: args.first() != Some(&"--list-langs") };
        let output_path = self.directory.path().join("output.txt");
        let output = File::create(&output_path).ok()?;
        let errors = File::create(self.directory.path().join("errors.txt")).ok()?;
        #[cfg(target_os = "linux")]
        let mut command = {
            let mut c = Command::new("nice");
            c.args(["-n", "10", "tesseract"]);
            c
        };
        #[cfg(not(target_os = "linux"))]
        let mut command = Command::new("tesseract");
        command.args(args).env("OMP_THREAD_LIMIT", "1").env("OMP_NUM_THREADS", "1").stdin(Stdio::null()).stdout(output).stderr(errors);
        if let Some(data) = &self.data {
            command.arg("--tessdata-dir").arg(data);
        }
        let mut child = command.spawn().ok()?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) => return None,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(None) => {}
            }
            let excessive_output = output_path.metadata().is_ok_and(|metadata| metadata.len() > evidence::MAX_SECTION_BYTES as u64) || self.directory.path().join("errors.txt").metadata().is_ok_and(|metadata| metadata.len() > 64 * 1024);
            if excessive_output || started.elapsed() >= PROCESS_BUDGET || self.started.elapsed() >= BOOK_BUDGET {
                let _ = child.kill();
                let _ = child.wait();
                self.status = if excessive_output { "output-limit" } else { "timeout" };
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut text = String::new();
        File::open(output_path).ok()?.take(evidence::MAX_SECTION_BYTES as u64 + 1).read_to_string(&mut text).ok()?;
        (text.len() <= evidence::MAX_SECTION_BYTES).then_some(text)
    }

    pub(crate) fn recognize(&mut self, bytes: &[u8], small_strip: bool) -> Option<String> {
        if bytes.len() > MAX_IMAGE_BYTES || !self.can_continue() {
            return None;
        }
        self.images += 1;
        let path = self.directory.path().join("input.image");
        File::create(&path).ok()?.write_all(bytes).ok()?;
        let args = [path.to_str()?, "stdout", "-l", "eng", "--psm", if small_strip { "6" } else { "3" }];
        let mut result = self.run(&args);
        // Tesseract sometimes treats solid coloured copyright pages as graphics.
        // Retry an empty result with a dark-text mask; never alter recognized digits.
        if result.as_ref().is_some_and(|text| text.trim().is_empty()) && self.can_continue() {
            if let Some(mask) = dark_text_mask(bytes) {
                File::create(&path).ok()?.write_all(&mask).ok()?;
                result = self.run(&args);
            }
        }
        if result.is_none() && self.status == "complete" {
            self.status = "recognition-error";
        }
        result
    }

    pub(crate) fn render_failed(&mut self) {
        self.status = "render-error";
    }
}

fn dark_text_mask(ppm: &[u8]) -> Option<Vec<u8>> {
    if !ppm.starts_with(b"P6\n") {
        return None;
    }
    let header_end = ppm.iter().enumerate().filter(|(_, value)| **value == b'\n').nth(2)?.0 + 1;
    let mut result = ppm[..header_end].to_vec();
    for pixel in ppm[header_end..].chunks_exact(3) {
        let luminance = (299 * u32::from(pixel[0]) + 587 * u32::from(pixel[1]) + 114 * u32::from(pixel[2])) / 1000;
        let value = if luminance < 64 { 0 } else { 255 };
        result.extend_from_slice(&[value; 3]);
    }
    Some(result)
}

pub(crate) fn inspect_epub(provider: &EpubProvider, book: &mut BookRecord) {
    let Some(mut session) = Session::start(book) else { return };
    if session.available() {
        let images = crate::epub_evidence::ocr_images(provider);
        for path in images {
            if !session.can_continue() || !needs_isbn(book) {
                break;
            }
            let bytes = provider.with_document_reader(&path, MAX_IMAGE_BYTES as u64 + 1, |reader| {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes)?;
                Ok(bytes)
            });
            let Ok(bytes) = bytes else { continue };
            let Some((pixels, small)) = prepare_image(&bytes) else { continue };
            if let Some(text) = session.recognize(&pixels, small) {
                let location = format!("epub:ocr:{path}");
                let section = inspect_image_text(book, &location, &text);
                evidence::apply_evidence(book, &[section], "epub");
            }
        }
    }
}

fn prepare_image(bytes: &[u8]) -> Option<(Vec<u8>, bool)> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_IMAGE_BYTES as u64);
    limits.max_image_width = Some(8000);
    limits.max_image_height = Some(8000);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    if image.width() < 40 || image.height() < 8 {
        return None;
    }
    let image = if image.width() < 800 { image.resize(image.width() * 3, image.height() * 3, image::imageops::FilterType::Triangle) } else { image };
    let image = image.thumbnail(image.width().min(2600), image.height().min(2600)).to_rgb8();
    let small = image.height() < 300;
    Some((ppm(image.width(), image.height(), image.as_raw().iter().copied()), small))
}

pub(crate) fn ppm(width: u32, height: u32, rgb: impl Iterator<Item = u8>) -> Vec<u8> {
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    bytes.extend(rgb);
    bytes
}

pub(crate) fn inspect_image_text(book: &BookRecord, location: &str, text: &str) -> SectionEvidence {
    // A selected image can be an ISBN strip without the surrounding copyright text.
    evidence::inspect_isbn_image(book, location, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_isbn_strips_are_upscaled_but_icon_noise_is_skipped() {
        let encode = |width, height| {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height));
            let mut output = std::io::Cursor::new(Vec::new());
            image.write_to(&mut output, image::ImageFormat::Png).unwrap();
            output.into_inner()
        };
        let (bytes, small) = prepare_image(&encode(200, 12)).unwrap();
        assert!(small);
        assert!(bytes.starts_with(b"P6\n600 36\n255\n"));
        assert!(prepare_image(&encode(12, 12)).is_none());
        assert!(prepare_image(&[0; 100]).is_none());
    }

    #[test]
    fn empty_result_mask_keeps_dark_text_on_saturated_backgrounds() {
        let bytes = ppm(3, 1, [0, 0, 0, 239, 0, 127, 200, 160, 0].into_iter());
        let mask = dark_text_mask(&bytes).unwrap();
        assert!(mask.ends_with(&[0, 0, 0, 255, 255, 255, 255, 255, 255]));
        assert!(dark_text_mask(b"not a raster").is_none());
    }

    #[test]
    fn existing_isbn_skips_ocr_but_lcc_alone_does_not() {
        let mut book = crate::fallback_metadata(std::path::Path::new("book.epub"));
        assert!(needs_isbn(&book));
        let section = evidence::inspect_section(&book, "copyright", "Copyright 2007\nRM231.2.H64 2007");
        evidence::apply_evidence(&mut book, &[section], "epub");
        assert!(needs_isbn(&book));
        let section = inspect_image_text(&book, "isbn.png", "ISBN 0-19-285458-5");
        evidence::apply_evidence(&mut book, &[section], "epub");
        assert!(!needs_isbn(&book));
    }
}
