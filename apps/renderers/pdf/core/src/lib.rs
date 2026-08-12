//! Framework-independent PDF sessions, rendering, searching, selection, and page caching.

#[cfg(all(feature = "packaged-pdfium", not(target_os = "linux")))]
compile_error!("the packaged-pdfium feature is only supported by Linux application packages");

use image::RgbaImage;
pub use pdf_view_common::{FitWidthColumn, FitWidthPage, FitWidthPlan, FitWidthPlanError, FitWidthViewport, NormalizedRect, PageSelection as PdfTextSelection, PageTextGeometry, PixelCrop, SelectionGranularity, SelectionOptions};
use pdf_view_common::{PageRotation, PageTransform, TextCharacter, merge_line_rects};
use pdfium_render::prelude::*;
pub use reader_style::{ReaderInteractionStyle, RgbaColor};
#[cfg(test)]
use std::collections::HashSet;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

#[cfg(not(feature = "packaged-pdfium"))]
const PDFIUM_BYTES: &[u8] = include_bytes!(env!("PDFIUM_EMBEDDED_PATH"));
#[cfg(feature = "packaged-pdfium")]
const PDFIUM_BUILD_LIBRARY_PATH: &str = env!("PDFIUM_BUILD_LIBRARY_PATH");
#[cfg(not(feature = "packaged-pdfium"))]
const PDFIUM_LIBRARY_NAME: &str = env!("PDFIUM_LIBRARY_NAME");
#[cfg(feature = "packaged-pdfium")]
const PDFIUM_PACKAGED_PATH: &str = env!("PDFIUM_PACKAGED_PATH");
const PDFIUM_THIRD_PARTY_NOTICES: &str = include_str!(env!("PDFIUM_NOTICES_PATH"));
#[cfg(not(feature = "packaged-pdfium"))]
const PDFIUM_VERSION: &str = "7961";
const DEFAULT_RENDER_WIDTH: u16 = 1400;
const MIN_RENDER_WIDTH: u16 = 320;
const MAX_RENDER_WIDTH: u16 = 3200;
#[cfg(test)]
const HEADER_ZONE_BOTTOM: f32 = 0.18;
#[cfg(test)]
const FOOTER_ZONE_TOP: f32 = 0.82;
const ELEMENT_CROP_PADDING: f32 = 0.008;

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

#[cfg(test)]
static ANALYZED_PAGE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub type PdfResult<T> = Result<T, PdfError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfError {
    message: String,
}

impl PdfError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl fmt::Display for PdfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PdfError {}

pub struct PdfDocumentSession {
    document: PdfDocument<'static>,
    info: PdfDocumentInfo,
}

/// A page raster without reader-only text, selection, or margin analysis.
///
/// This is intentionally smaller than [`RenderedPdfPage`] so callers such as
/// cover generation do not pay the cost of constructing a full reader session.
#[derive(Clone, Debug)]
pub struct RenderedPdfImage {
    rgba: Vec<u8>,
    pixel_width: u32,
    pixel_height: u32,
    source: RenderedPdfImageSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderedPdfImageSource {
    EmbeddedThumbnail,
    RenderedPage,
}

impl RenderedPdfImage {
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn pixel_width(&self) -> u32 {
        self.pixel_width
    }

    pub fn pixel_height(&self) -> u32 {
        self.pixel_height
    }

    pub fn source(&self) -> RenderedPdfImageSource {
        self.source
    }

    pub fn into_parts(self) -> (Vec<u8>, u32, u32) {
        (self.rgba, self.pixel_width, self.pixel_height)
    }
}

/// Opens a PDF and renders only its first page.
///
/// Unlike [`PdfDocumentSession::open`], this does not inspect every page for
/// reader metadata, text geometry, repeating headers, or trim bounds.
pub fn render_first_page_image(path: impl AsRef<Path>, target_width: u16) -> PdfResult<RenderedPdfImage> {
    let path = path.as_ref();
    let document = shared_pdfium()?.load_pdf_from_file(path, None).map_err(|error| PdfError::new(format!("opening {}: {error}", path.display())))?;
    let page = document.pages().get(0).map_err(|_| PdfError::new("the PDF has no pages"))?;
    if let Ok(bitmap) = page.embedded_thumbnail()
        && !matches!(bitmap.format(), Ok(PdfBitmapFormat::Gray))
    {
        let pixel_width = bitmap.width();
        let pixel_height = bitmap.height();
        let rgba = bitmap.as_rgba_bytes();
        if pixel_width > 0 && pixel_height > 0 && rgba.len() == pixel_width as usize * pixel_height as usize * 4 {
            return Ok(RenderedPdfImage { rgba, pixel_width: pixel_width as u32, pixel_height: pixel_height as u32, source: RenderedPdfImageSource::EmbeddedThumbnail });
        }
    }
    let width = target_width.clamp(1, MAX_RENDER_WIDTH) as i32;
    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(width).render_annotations(true)).map_err(|error| PdfError::new(format!("rendering PDF page 1: {error}")))?;
    Ok(RenderedPdfImage { rgba: bitmap.as_rgba_bytes(), pixel_width: bitmap.width() as u32, pixel_height: bitmap.height() as u32, source: RenderedPdfImageSource::RenderedPage })
}

#[derive(Clone, Debug)]
struct PageTextLine {
    #[cfg(test)]
    text: String,
    bounds: NormalizedRect,
}

#[derive(Clone, Debug, Default)]
struct PageElements {
    text_lines: Vec<PageTextLine>,
    graphics: Vec<NormalizedRect>,
}

#[derive(Clone, Copy, Debug, Default)]
struct RepeatingPageBands {
    header_bottom: Option<f32>,
    footer_top: Option<f32>,
}

/// Adapts Rust's short-read semantics to Pdfium's custom file-access contract.
///
/// `pdfium-render` treats any non-zero return from `Read::read()` as a successful
/// read of the complete requested block. A conforming reader may return fewer
/// bytes without reaching EOF, particularly when its data comes from a remote
/// or buffered provider. Make each callback read all-or-error so Pdfium never
/// parses an only partially initialized block.
struct PdfiumExactReader<R> {
    inner: R,
}

impl<R> PdfiumExactReader<R> {
    fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R: Read> Read for PdfiumExactReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read_exact(buffer)?;
        Ok(buffer.len())
    }
}

impl<R: Seek> Seek for PdfiumExactReader<R> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

impl PdfDocumentSession {
    pub fn open(path: impl Into<PathBuf>) -> PdfResult<Self> {
        let path = path.into();
        let bytes = fs::read(&path).map_err(|error| PdfError::new(format!("reading {}: {error}", path.display())))?;
        Self::from_bytes(path, bytes)
    }

    pub fn from_bytes(display_path: impl Into<PathBuf>, bytes: Vec<u8>) -> PdfResult<Self> {
        let path = display_path.into();
        let document = shared_pdfium()?.load_pdf_from_byte_vec(bytes, None).map_err(|error| PdfError::new(format!("opening {}: {error}", path.display())))?;
        Self::from_document(path, document)
    }

    pub fn from_reader<R>(display_path: impl Into<PathBuf>, reader: R) -> PdfResult<Self>
    where
        R: Read + Seek + 'static,
    {
        let path = display_path.into();
        let document = shared_pdfium()?.load_pdf_from_reader(PdfiumExactReader::new(reader), None).map_err(|error| PdfError::new(format!("opening {}: {error}", path.display())))?;
        Self::from_document(path, document)
    }

    fn from_document(path: PathBuf, document: PdfDocument<'static>) -> PdfResult<Self> {
        let pages = document.pages().iter().map(|page| PdfPageInfo { width_points: page.width().value, height_points: page.height().value }).collect::<Vec<_>>();
        if pages.is_empty() {
            return Err(PdfError::new("the PDF has no pages"));
        }
        let outline = document
            .bookmarks()
            .iter()
            .filter_map(|bookmark| {
                let title = bookmark.title()?.trim().to_owned();
                if title.is_empty() {
                    return None;
                }
                let page_index = bookmark.destination().and_then(|destination| destination.page_index().ok()).or_else(|| bookmark.action()?.as_local_destination_action()?.destination().ok()?.page_index().ok())? as usize;
                let mut depth = 0;
                let mut parent = bookmark.parent();
                while let Some(ancestor) = parent {
                    depth += 1;
                    parent = ancestor.parent();
                }
                Some(PdfOutlineEntry { title, page_index, depth })
            })
            .collect();
        Ok(Self { document, info: PdfDocumentInfo { path, pages, outline } })
    }

    pub fn info(&self) -> &PdfDocumentInfo {
        &self.info
    }

    pub fn render_page(&self, page_index: usize, target_width: Option<u16>) -> PdfResult<RenderedPdfPage> {
        self.render_page_with_margin_trim(page_index, target_width, false)
    }

    pub fn render_page_with_margin_trim(&self, page_index: usize, target_width: Option<u16>, trim_margins: bool) -> PdfResult<RenderedPdfPage> {
        let page = self.document.pages().get(page_index as i32).map_err(|_| PdfError::new(format!("PDF page {} does not exist", page_index + 1)))?;
        let width = target_width.unwrap_or(DEFAULT_RENDER_WIDTH).clamp(MIN_RENDER_WIDTH, MAX_RENDER_WIDTH) as i32;
        let mut text_layout = extract_text_layout(&page, page_index)?;
        let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(width).render_annotations(true)).map_err(|error| PdfError::new(format!("rendering PDF page {}: {error}", page_index + 1)))?;
        let pixel_width = bitmap.width() as u32;
        let pixel_height = bitmap.height() as u32;
        let rgba = bitmap.as_rgba_bytes();
        let crop = if trim_margins {
            let elements = analyze_page_elements(&page, page_index);
            content_bounds(&elements, RepeatingPageBands::default()).map(|bounds| pixel_crop_from_normalized(bounds, pixel_width, pixel_height)).unwrap_or_else(|| PixelCrop::full(pixel_width, pixel_height))
        } else {
            PixelCrop::full(pixel_width, pixel_height)
        };
        let image = RgbaImage::from_raw(pixel_width, pixel_height, rgba).ok_or_else(|| PdfError::new(format!("rendering PDF page {} returned an invalid RGBA buffer", page_index + 1)))?;
        let rgba = if crop.is_full_page() { image.into_raw() } else { image::imageops::crop_imm(&image, crop.left(), crop.top(), crop.width(), crop.height()).to_image().into_raw() };
        if trim_margins {
            text_layout.apply_crop(crop);
        }
        let image_bands = extract_image_break_avoidance(&page, crop);
        let break_avoidance = sorted_vertical_bands(text_layout.break_avoidance_bands().into_iter().chain(image_bands));
        Ok(RenderedPdfPage { rgba, pixel_width: crop.width(), pixel_height: crop.height(), crop, text_layout: Arc::new(text_layout), break_avoidance: break_avoidance.into() })
    }

    pub fn search(&self, query: &str) -> PdfResult<Vec<PdfSearchHit>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.document.pages().iter().enumerate().try_fold(Vec::new(), |mut hits, (page_index, page)| {
            let page_width = page.width().value.max(1.0);
            let page_height = page.height().value.max(1.0);
            let text = page.text().map_err(|error| PdfError::new(format!("extracting text from page {}: {error}", page_index + 1)))?;
            let search = text.search(query, &PdfSearchOptions::new()).map_err(|error| PdfError::new(format!("searching page {}: {error}", page_index + 1)))?;
            search.iter(PdfSearchDirection::SearchForward).for_each(|segments| {
                segments.iter().for_each(|segment| {
                    hits.push(PdfSearchHit { page_index, bounds: normalized_pdf_bounds(segment.bounds(), page_width, page_height) });
                });
            });
            Ok(hits)
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfDocumentInfo {
    path: PathBuf,
    pages: Vec<PdfPageInfo>,
    outline: Vec<PdfOutlineEntry>,
}

impl PdfDocumentInfo {
    pub fn open(path: impl Into<PathBuf>) -> PdfResult<Self> {
        Ok(PdfDocumentSession::open(path)?.info)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn page(&self, page_index: usize) -> Option<&PdfPageInfo> {
        self.pages.get(page_index)
    }

    pub fn pages(&self) -> &[PdfPageInfo] {
        &self.pages
    }

    pub fn outline(&self) -> &[PdfOutlineEntry] {
        &self.outline
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfOutlineEntry {
    title: String,
    page_index: usize,
    depth: usize,
}

impl PdfOutlineEntry {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn page_index(&self) -> usize {
        self.page_index
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfPageInfo {
    width_points: f32,
    height_points: f32,
}

impl PdfPageInfo {
    pub fn width_points(&self) -> f32 {
        self.width_points
    }

    pub fn height_points(&self) -> f32 {
        self.height_points
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.width_points / self.height_points.max(1.0)
    }
}

#[derive(Clone, Debug)]
pub struct RenderedPdfPage {
    rgba: Vec<u8>,
    pixel_width: u32,
    pixel_height: u32,
    crop: PixelCrop,
    text_layout: Arc<PdfPageTextLayout>,
    break_avoidance: Arc<[(f32, f32)]>,
}

impl RenderedPdfPage {
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn pixel_width(&self) -> u32 {
        self.pixel_width
    }

    pub fn pixel_height(&self) -> u32 {
        self.pixel_height
    }

    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }

    pub fn crop(&self) -> PixelCrop {
        self.crop
    }

    pub fn break_avoidance(&self) -> &[(f32, f32)] {
        &self.break_avoidance
    }

    pub fn select_text(&self, start: (f32, f32), end: (f32, f32)) -> Option<PdfTextSelection> {
        self.text_layout.select(start, end)
    }

    pub fn into_parts(self) -> (Vec<u8>, u32, u32, PixelCrop, Arc<PdfPageTextLayout>, Arc<[(f32, f32)]>) {
        (self.rgba, self.pixel_width, self.pixel_height, self.crop, self.text_layout, self.break_avoidance)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PdfPageTextLayout {
    page_index: usize,
    geometry: PageTextGeometry,
}

impl PdfPageTextLayout {
    pub fn select(&self, start: (f32, f32), end: (f32, f32)) -> Option<PdfTextSelection> {
        let _ = self.page_index;
        self.geometry.select(start, end)
    }

    pub fn select_with_options(&self, start: (f32, f32), end: (f32, f32), options: SelectionOptions) -> Option<PdfTextSelection> {
        let _ = self.page_index;
        self.geometry.select_with_options(start, end, options)
    }

    pub fn geometry(&self) -> &PageTextGeometry {
        &self.geometry
    }

    fn apply_crop(&mut self, crop: PixelCrop) {
        self.geometry = self.geometry.transformed(PageTransform::new(crop, PageRotation::None));
    }

    fn break_avoidance_bands(&self) -> Vec<(f32, f32)> {
        merge_line_rects(self.geometry.characters().iter().filter(|character| !character.character().is_control() && !character.character().is_whitespace()).map(|character| character.bounds()))
            .into_iter()
            .map(|line| ((line.top() - 0.002).max(0.0), (line.bottom() + 0.002).min(1.0)))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfSearchHit {
    page_index: usize,
    bounds: NormalizedRect,
}

impl PdfSearchHit {
    pub fn page_index(&self) -> usize {
        self.page_index
    }

    pub fn bounds(&self) -> NormalizedRect {
        self.bounds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageRenderKey {
    page_index: usize,
    target_width: u16,
    trim_margins: bool,
}

impl PageRenderKey {
    pub fn new(page_index: usize, target_width: u16) -> Self {
        Self::with_margin_trim(page_index, target_width, false)
    }

    pub fn with_margin_trim(page_index: usize, target_width: u16, trim_margins: bool) -> Self {
        Self { page_index, target_width: target_width.clamp(MIN_RENDER_WIDTH, MAX_RENDER_WIDTH), trim_margins }
    }

    pub fn page_index(&self) -> usize {
        self.page_index
    }

    pub fn target_width(&self) -> u16 {
        self.target_width
    }

    pub fn trim_margins(&self) -> bool {
        self.trim_margins
    }
}

pub struct PageRenderCache {
    byte_budget: usize,
    byte_len: usize,
    pages: HashMap<PageRenderKey, Arc<RenderedPdfPage>>,
    recency: VecDeque<PageRenderKey>,
}

impl PageRenderCache {
    pub fn new(byte_budget: usize) -> Self {
        Self { byte_budget, byte_len: 0, pages: HashMap::new(), recency: VecDeque::new() }
    }

    pub fn get(&mut self, key: PageRenderKey) -> Option<Arc<RenderedPdfPage>> {
        let page = self.pages.get(&key)?.clone();
        self.promote(key);
        Some(page)
    }

    pub fn insert(&mut self, key: PageRenderKey, page: Arc<RenderedPdfPage>) {
        if page.byte_len() > self.byte_budget {
            return;
        }
        if let Some(previous) = self.pages.remove(&key) {
            self.byte_len = self.byte_len.saturating_sub(previous.byte_len());
            self.recency.retain(|candidate| *candidate != key);
        }
        while self.byte_len + page.byte_len() > self.byte_budget {
            let Some(oldest) = self.recency.pop_front() else {
                break;
            };
            if let Some(evicted) = self.pages.remove(&oldest) {
                self.byte_len = self.byte_len.saturating_sub(evicted.byte_len());
            }
        }
        self.byte_len += page.byte_len();
        self.pages.insert(key, page);
        self.recency.push_back(key);
    }

    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    fn promote(&mut self, key: PageRenderKey) {
        self.recency.retain(|candidate| *candidate != key);
        self.recency.push_back(key);
    }
}

pub fn is_pdf_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

pub fn pdfium_third_party_notices() -> &'static str {
    PDFIUM_THIRD_PARTY_NOTICES
}

fn extract_text_layout(page: &PdfPage<'_>, page_index: usize) -> PdfResult<PdfPageTextLayout> {
    let width = page.width().value.max(1.0);
    let height = page.height().value.max(1.0);
    let text = page.text().map_err(|error| PdfError::new(format!("extracting text from page {}: {error}", page_index + 1)))?;
    let characters = text
        .chars()
        .iter()
        .filter_map(|character| {
            let unicode = character.unicode_char()?;
            let tight = character.tight_bounds().ok();
            let loose = character.loose_bounds().ok();
            let bounds = match (tight, loose) {
                // Tight side bearings align with the painted glyph. The loose
                // ascent and descent produce the expected full-height selection.
                (Some(tight), Some(loose)) => PdfRect::new(loose.bottom(), tight.left(), loose.top(), tight.right()),
                (Some(bounds), None) | (None, Some(bounds)) => bounds,
                (None, None) => return None,
            };
            Some(TextCharacter::new(character.index(), unicode, normalized_pdf_bounds(bounds, width, height)))
        })
        .collect::<Vec<_>>();
    Ok(PdfPageTextLayout { page_index, geometry: PageTextGeometry::new(characters) })
}

fn analyze_page_elements(page: &PdfPage<'_>, page_index: usize) -> PageElements {
    #[cfg(test)]
    ANALYZED_PAGE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let page_width = page.width().value.max(1.0);
    let page_height = page.height().value.max(1.0);
    let text_lines = extract_text_layout(page, page_index).map(|layout| text_lines(layout.geometry())).unwrap_or_default();
    let mut graphics = Vec::new();
    for object in page.objects().iter() {
        collect_graphic_bounds(&object, page_width, page_height, &mut graphics);
    }
    PageElements { text_lines, graphics }
}

fn collect_graphic_bounds(object: &PdfPageObject<'_>, page_width: f32, page_height: f32, graphics: &mut Vec<NormalizedRect>) {
    if object.as_text_object().is_some() {
        return;
    }
    if let Some(form) = object.as_x_object_form_object() {
        for child in form.iter() {
            collect_graphic_bounds(&child, page_width, page_height, graphics);
        }
        return;
    }
    let Some(bounds) = object.bounds().ok().map(|bounds| normalized_pdf_bounds(bounds.to_rect(), page_width, page_height)) else {
        return;
    };
    let is_page_background = bounds.width() >= 0.98 && bounds.height() >= 0.98;
    let is_margin_rule = (bounds.width() >= 0.5 && bounds.height() <= 0.006) || (bounds.height() >= 0.5 && bounds.width() <= 0.006);
    if !is_page_background && !is_margin_rule && bounds.width() > 0.0001 && bounds.height() > 0.0001 {
        graphics.push(bounds);
    }
}

fn text_lines(geometry: &PageTextGeometry) -> Vec<PageTextLine> {
    let mut lines = Vec::new();
    let mut text = String::new();
    let mut bounds: Option<NormalizedRect> = None;
    for character in geometry.characters() {
        let value = character.character();
        if value.is_control() {
            push_text_line(&mut lines, &mut text, &mut bounds);
            continue;
        }
        if value.is_whitespace() {
            if !text.ends_with(' ') && !text.is_empty() {
                text.push(' ');
            }
            continue;
        }
        let character_bounds = character.bounds();
        if bounds.is_some_and(|line_bounds| !same_visual_text_line(line_bounds, character_bounds)) {
            push_text_line(&mut lines, &mut text, &mut bounds);
        }
        text.push(value);
        bounds = Some(bounds.map_or(character_bounds, |line_bounds| line_bounds.union(character_bounds)));
    }
    push_text_line(&mut lines, &mut text, &mut bounds);
    lines
}

fn push_text_line(lines: &mut Vec<PageTextLine>, text: &mut String, bounds: &mut Option<NormalizedRect>) {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(bounds) = bounds.take().filter(|_| !normalized.is_empty()) {
        lines.push(PageTextLine {
            #[cfg(test)]
            text: normalized,
            bounds,
        });
    }
    text.clear();
}

fn same_visual_text_line(left: NormalizedRect, right: NormalizedRect) -> bool {
    let vertical_overlap = left.bottom().min(right.bottom()) - left.top().max(right.top());
    let horizontal_gap = if left.right() < right.left() {
        right.left() - left.right()
    } else if right.right() < left.left() {
        left.left() - right.right()
    } else {
        0.0
    };
    vertical_overlap > left.height().min(right.height()) * 0.3 && horizontal_gap <= 0.08
}

#[cfg(test)]
fn repeating_page_bands(pages: &[PageElements]) -> Vec<RepeatingPageBands> {
    let required_pages = ((pages.len() as f32 * 0.4).ceil() as usize).max(2);
    if pages.len() < required_pages {
        return vec![RepeatingPageBands::default(); pages.len()];
    }
    pages
        .iter()
        .enumerate()
        .map(|(page_index, page)| {
            let mut bands = RepeatingPageBands::default();
            for line in &page.text_lines {
                let zone = line_zone(line.bounds);
                let Some(zone) = zone else { continue };
                let matching_pages = pages
                    .iter()
                    .enumerate()
                    .filter(|(candidate_page, _)| *candidate_page != page_index)
                    .filter(|(_, candidate)| candidate.text_lines.iter().any(|candidate| line_zone(candidate.bounds) == Some(zone) && vertically_aligned(line.bounds, candidate.bounds) && text_overlap(&line.text, &candidate.text) >= 0.8))
                    .count()
                    + 1;
                if matching_pages < required_pages {
                    continue;
                }
                match zone {
                    PageLineZone::Header => bands.header_bottom = Some(bands.header_bottom.unwrap_or(0.0).max((line.bounds.bottom() + 0.01).min(1.0))),
                    PageLineZone::Footer => bands.footer_top = Some(bands.footer_top.unwrap_or(1.0).min((line.bounds.top() - 0.01).max(0.0))),
                }
            }
            bands
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
enum PageLineZone {
    Header,
    Footer,
}

#[cfg(test)]
fn line_zone(bounds: NormalizedRect) -> Option<PageLineZone> {
    let center = bounds.top() + bounds.height() * 0.5;
    if center <= HEADER_ZONE_BOTTOM {
        Some(PageLineZone::Header)
    } else if center >= FOOTER_ZONE_TOP {
        Some(PageLineZone::Footer)
    } else {
        None
    }
}

#[cfg(test)]
fn vertically_aligned(left: NormalizedRect, right: NormalizedRect) -> bool {
    let overlap = left.bottom().min(right.bottom()) - left.top().max(right.top());
    let overlap_ratio = overlap.max(0.0) / left.height().min(right.height()).max(f32::EPSILON);
    let center_distance = ((left.top() + left.height() * 0.5) - (right.top() + right.height() * 0.5)).abs();
    overlap_ratio >= 0.5 || center_distance <= 0.012
}

#[cfg(test)]
fn text_overlap(left: &str, right: &str) -> f32 {
    let left = normalized_text_tokens(left);
    let right = normalized_text_tokens(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let common = left.intersection(&right).count() as f32;
    2.0 * common / (left.len() + right.len()) as f32
}

#[cfg(test)]
fn normalized_text_tokens(text: &str) -> HashSet<String> {
    text.to_lowercase().split(|character: char| !character.is_alphanumeric()).filter(|token| !token.is_empty()).map(|token| if token.chars().all(|character| character.is_ascii_digit()) { "#".to_owned() } else { token.to_owned() }).collect()
}

fn content_bounds(elements: &PageElements, bands: RepeatingPageBands) -> Option<NormalizedRect> {
    let is_body = |bounds: NormalizedRect| {
        let center = bounds.top() + bounds.height() * 0.5;
        bands.header_bottom.is_none_or(|bottom| center >= bottom) && bands.footer_top.is_none_or(|top| center <= top)
    };
    let mut bounds = elements.text_lines.iter().map(|line| line.bounds).chain(elements.graphics.iter().copied()).filter(|bounds| is_body(*bounds)).reduce(NormalizedRect::union)?;
    let left = (bounds.left() - ELEMENT_CROP_PADDING).max(0.0);
    let top = (bounds.top() - ELEMENT_CROP_PADDING).max(bands.header_bottom.unwrap_or(0.0));
    let right = (bounds.right() + ELEMENT_CROP_PADDING).min(1.0);
    let bottom = (bounds.bottom() + ELEMENT_CROP_PADDING).min(bands.footer_top.unwrap_or(1.0));
    if right <= left || bottom <= top {
        return None;
    }
    bounds = NormalizedRect::new(left, top, right - left, bottom - top);
    Some(bounds)
}

fn pixel_crop_from_normalized(bounds: NormalizedRect, width: u32, height: u32) -> PixelCrop {
    let left = (bounds.left() * width as f32).floor() as u32;
    let top = (bounds.top() * height as f32).floor() as u32;
    let right = (bounds.right() * width as f32).ceil() as u32;
    let bottom = (bounds.bottom() * height as f32).ceil() as u32;
    PixelCrop::new(width, height, left, top, right.saturating_sub(left), bottom.saturating_sub(top))
}

fn normalized_pdf_bounds(bounds: PdfRect, page_width: f32, page_height: f32) -> NormalizedRect {
    NormalizedRect::new(bounds.left().value / page_width, 1.0 - bounds.top().value / page_height, bounds.width().value / page_width, bounds.height().value / page_height)
}

fn extract_image_break_avoidance(page: &PdfPage<'_>, crop: PixelCrop) -> Vec<(f32, f32)> {
    let page_width = page.width().value.max(1.0);
    let page_height = page.height().value.max(1.0);
    page.objects()
        .iter()
        .filter(|object| object.as_image_object().is_some())
        .filter_map(|object| object.bounds().ok().map(|bounds| bounds.to_rect()))
        .filter_map(|bounds| crop.full_rect_to_display(normalized_pdf_bounds(bounds, page_width, page_height)))
        .map(|bounds| ((bounds.top() - 0.002).max(0.0), (bounds.bottom() + 0.002).min(1.0)))
        .collect()
}

fn sorted_vertical_bands(bands: impl IntoIterator<Item = (f32, f32)>) -> Vec<(f32, f32)> {
    let mut bands = bands.into_iter().filter(|(top, bottom)| top.is_finite() && bottom.is_finite() && bottom > top).collect::<Vec<_>>();
    bands.sort_by(|left, right| left.0.total_cmp(&right.0));
    bands
}

pub fn crop_normalized_rect(crop: PixelCrop, rect: NormalizedRect) -> Option<NormalizedRect> {
    crop.full_rect_to_display(rect)
}

fn shared_pdfium() -> PdfResult<&'static Pdfium> {
    let pdfium = PDFIUM.get_or_init(|| {
        let path = pdfium_library_path().map_err(|error| error.to_string())?;
        let bindings = Pdfium::bind_to_library(&path).map_err(|error| format!("loading PDFium from {}: {error}", path.display()))?;
        Ok(Pdfium::new(bindings))
    });
    pdfium.as_ref().map_err(|error| PdfError::new(error.clone()))
}

#[cfg(feature = "packaged-pdfium")]
fn pdfium_library_path() -> PdfResult<PathBuf> {
    let path = if let Some(path) = std::env::var_os("BOKHEIM_PDFIUM_LIBRARY_PATH") {
        PathBuf::from(path)
    } else if cfg!(test) {
        // Unit tests run before a package has been assembled, so use the
        // verified build input. Installed applications use the immutable path.
        PathBuf::from(PDFIUM_BUILD_LIBRARY_PATH)
    } else {
        PathBuf::from(PDFIUM_PACKAGED_PATH)
    };
    if !path.is_file() {
        return Err(PdfError::new(format!("packaged PDFium library is missing: {}", path.display())));
    }
    Ok(path)
}

#[cfg(not(feature = "packaged-pdfium"))]
fn pdfium_library_path() -> PdfResult<PathBuf> {
    extracted_pdfium_path()
}

#[cfg(not(feature = "packaged-pdfium"))]
fn extracted_pdfium_path() -> PdfResult<PathBuf> {
    let directory = std::env::temp_dir().join("bokheim-pdf-reader").join(PDFIUM_VERSION);
    let path = directory.join(PDFIUM_LIBRARY_NAME);
    if path.metadata().is_ok_and(|metadata| metadata.len() == PDFIUM_BYTES.len() as u64) {
        return Ok(path);
    }
    fs::create_dir_all(&directory).map_err(|error| PdfError::new(format!("creating PDFium cache directory: {error}")))?;
    let temporary = directory.join(format!("{PDFIUM_LIBRARY_NAME}-{}.partial", std::process::id()));
    fs::write(&temporary, PDFIUM_BYTES).map_err(|error| PdfError::new(format!("extracting embedded PDFium: {error}")))?;
    match fs::rename(&temporary, &path) {
        Ok(()) => Ok(path),
        Err(_) if path.exists() => {
            let _ = fs::remove_file(&temporary);
            Ok(path)
        }
        Err(error) => Err(PdfError::new(format!("installing embedded PDFium: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    static PDFIUM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn fixture_path(label: &str) -> PdfResult<PathBuf> {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| PdfError::new(error.to_string()))?.as_nanos();
        Ok(std::env::temp_dir().join(format!("bokheim-pdf-{label}-{suffix}.pdf")))
    }

    fn create_fixture_pdf(path: &Path) -> PdfResult<()> {
        let pdfium = shared_pdfium()?;
        let mut document = pdfium.create_new_pdf().map_err(|error| PdfError::new(error.to_string()))?;
        let font = document.fonts_mut().helvetica();
        for text in ["First searchable page", "Second searchable page"] {
            let mut page = document.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).map_err(|error| PdfError::new(error.to_string()))?;
            page.objects_mut().create_text_object(PdfPoints::new(72.0), PdfPoints::new(720.0), text, font, PdfPoints::new(18.0)).map_err(|error| PdfError::new(error.to_string()))?;
        }
        document.save_to_file(path).map_err(|error| PdfError::new(error.to_string()))
    }

    struct ChunkedReader<R> {
        inner: R,
        chunk_size: usize,
    }

    impl<R: Read> Read for ChunkedReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let chunk_size = self.chunk_size.min(buffer.len());
            self.inner.read(&mut buffer[..chunk_size])
        }
    }

    impl<R: Seek> Seek for ChunkedReader<R> {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(position)
        }
    }

    fn fake_page(byte_len: usize) -> Arc<RenderedPdfPage> {
        Arc::new(RenderedPdfPage {
            rgba: vec![0; byte_len],
            pixel_width: 1,
            pixel_height: 1,
            crop: PixelCrop::full(1, 1),
            text_layout: Arc::new(PdfPageTextLayout { page_index: 0, geometry: PageTextGeometry::default() }),
            break_avoidance: Arc::from([]),
        })
    }

    #[test]
    fn page_render_keys_bound_requested_width() {
        assert_eq!(PageRenderKey::new(2, 1).target_width(), MIN_RENDER_WIDTH);
        assert_eq!(PageRenderKey::new(2, u16::MAX).target_width(), MAX_RENDER_WIDTH);
        assert_ne!(PageRenderKey::new(2, 800), PageRenderKey::with_margin_trim(2, 800, true));
    }

    #[test]
    fn page_cache_evicts_least_recently_used_pages() {
        let mut cache = PageRenderCache::new(20);
        let first = PageRenderKey::new(0, 800);
        let second = PageRenderKey::new(1, 800);
        let third = PageRenderKey::new(2, 800);
        cache.insert(first, fake_page(10));
        cache.insert(second, fake_page(10));
        assert!(cache.get(first).is_some());
        cache.insert(third, fake_page(10));
        assert!(cache.get(first).is_some());
        assert!(cache.get(second).is_none());
        assert!(cache.get(third).is_some());
        assert_eq!(cache.byte_len(), 20);
    }

    #[test]
    fn oversized_page_is_not_cached() {
        let mut cache = PageRenderCache::new(5);
        cache.insert(PageRenderKey::new(0, 800), fake_page(6));
        assert!(cache.is_empty());
    }

    #[test]
    fn selection_rects_merge_characters_on_the_same_line() {
        let layout = PdfPageTextLayout { page_index: 0, geometry: PageTextGeometry::new([TextCharacter::new(0, 'A', NormalizedRect::new(0.1, 0.2, 0.04, 0.03)), TextCharacter::new(1, 'B', NormalizedRect::new(0.14, 0.2, 0.04, 0.03))]) };
        let selection = layout.select((0.11, 0.21), (0.17, 0.21)).expect("text selection");
        assert_eq!(selection.text(), "AB");
        assert_eq!(selection.rects().len(), 1);
        assert!((selection.rects()[0].width() - 0.08).abs() < 0.0001);
    }

    #[test]
    fn embedded_pdfium_session_opens_once_renders_searches_and_selects() -> PdfResult<()> {
        let _pdfium_guard = PDFIUM_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = fixture_path("reader")?;
        create_fixture_pdf(&path)?;
        let session = PdfDocumentSession::open(&path)?;
        assert_eq!(session.info().page_count(), 2);
        assert!(session.info().pages().iter().all(|page| page.aspect_ratio() > 0.0));
        let rendered = session.render_page(1, Some(640))?;
        assert_eq!(rendered.pixel_width(), 640);
        assert!(rendered.pixel_height() > rendered.pixel_width());
        assert_eq!(rendered.rgba().len(), rendered.pixel_width() as usize * rendered.pixel_height() as usize * 4);
        assert!(!rendered.break_avoidance().is_empty(), "text lines should protect fit-width slice boundaries");
        let hits = session.search("Second searchable")?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_index(), 1);
        let hit = hits[0].bounds();
        let selection = rendered.select_text((hit.left() + 0.001, hit.top() + hit.height() * 0.5), (hit.left() + hit.width(), hit.top() + hit.height() * 0.5)).expect("selection from search bounds");
        assert!(selection.text().contains("Second searchable"));

        let trimmed = session.render_page_with_margin_trim(1, Some(640), true)?;
        assert!(!trimmed.crop().is_full_page(), "element trim bounds should crop the rendered page");
        assert!(trimmed.pixel_width() < trimmed.crop().full_width());
        assert_eq!(trimmed.rgba().len(), trimmed.pixel_width() as usize * trimmed.pixel_height() as usize * 4);
        let mapped = crop_normalized_rect(trimmed.crop(), hit).expect("search hit remains inside the trimmed page");
        let trimmed_selection = trimmed.select_text((mapped.left() + 0.001, mapped.top() + mapped.height() * 0.5), (mapped.left() + mapped.width(), mapped.top() + mapped.height() * 0.5)).expect("selection from trimmed search bounds");
        assert!(trimmed_selection.text().contains("Second searchable"));

        drop(session);
        fs::remove_file(path).map_err(|error| PdfError::new(error.to_string()))?;
        Ok(())
    }

    #[test]
    fn first_page_image_skips_document_wide_reader_analysis() -> PdfResult<()> {
        let _pdfium_guard = PDFIUM_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = fixture_path("first-page-image")?;
        create_fixture_pdf(&path)?;
        ANALYZED_PAGE_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);

        let rendered = render_first_page_image(&path, MIN_RENDER_WIDTH)?;

        assert_eq!(rendered.source(), RenderedPdfImageSource::RenderedPage);
        assert_eq!(rendered.pixel_width(), MIN_RENDER_WIDTH as u32);
        assert!(rendered.pixel_height() > rendered.pixel_width());
        assert_eq!(rendered.rgba().len(), rendered.pixel_width() as usize * rendered.pixel_height() as usize * 4);
        assert_eq!(ANALYZED_PAGE_COUNT.load(std::sync::atomic::Ordering::Relaxed), 0);

        ANALYZED_PAGE_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
        let session = PdfDocumentSession::open(&path)?;
        assert_eq!(ANALYZED_PAGE_COUNT.load(std::sync::atomic::Ordering::Relaxed), 0, "opening the reader must not analyze every page before page 1 can render");
        let reader_rendered = session.render_page(0, Some(300))?;
        assert_eq!(rendered.pixel_width(), reader_rendered.pixel_width());
        assert_eq!(rendered.pixel_height(), reader_rendered.pixel_height());
        assert_eq!(rendered.rgba(), reader_rendered.rgba());
        assert_eq!(ANALYZED_PAGE_COUNT.load(std::sync::atomic::Ordering::Relaxed), 0);

        let _ = session.render_page_with_margin_trim(0, Some(300), true)?;
        assert_eq!(ANALYZED_PAGE_COUNT.load(std::sync::atomic::Ordering::Relaxed), 1, "margin analysis should be limited to the page being rendered");

        drop(session);
        fs::remove_file(path).map_err(|error| PdfError::new(error.to_string()))?;
        Ok(())
    }

    #[test]
    fn reader_short_reads_are_completed_before_pdfium_parses_them() -> PdfResult<()> {
        let _pdfium_guard = PDFIUM_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = fixture_path("short-reads")?;
        create_fixture_pdf(&path)?;
        let bytes = fs::read(&path).map_err(|error| PdfError::new(error.to_string()))?;
        let reader = ChunkedReader { inner: Cursor::new(bytes), chunk_size: 7 };
        let session = PdfDocumentSession::from_reader(&path, reader)?;
        assert_eq!(session.info().page_count(), 2);

        drop(session);
        fs::remove_file(path).map_err(|error| PdfError::new(error.to_string()))?;
        Ok(())
    }

    #[test]
    fn vertical_break_avoidance_bands_stay_independent() {
        assert_eq!(sorted_vertical_bands([(0.5, 0.7), (0.1, 0.2), (0.18, 0.3), (0.8, 0.9)]), vec![(0.1, 0.2), (0.18, 0.3), (0.5, 0.7), (0.8, 0.9)]);
    }

    #[test]
    fn text_break_avoidance_groups_characters_by_line_only() {
        let layout = PdfPageTextLayout {
            page_index: 0,
            geometry: PageTextGeometry::new([
                TextCharacter::new(0, 'A', NormalizedRect::new(0.1, 0.20, 0.04, 0.03)),
                TextCharacter::new(1, 'B', NormalizedRect::new(0.14, 0.20, 0.04, 0.03)),
                TextCharacter::new(2, 'C', NormalizedRect::new(0.1, 0.25, 0.04, 0.03)),
            ]),
        };
        assert_eq!(layout.break_avoidance_bands().len(), 2);
    }

    #[test]
    fn repeating_aligned_headers_and_footers_are_excluded_from_element_bounds() {
        let page = |page_number: usize, body_top: f32| PageElements {
            text_lines: vec![
                PageTextLine { text: "Scientific Reports | nature.com".to_owned(), bounds: NormalizedRect::new(0.08, 0.03, 0.5, 0.025) },
                PageTextLine { text: format!("Page {page_number}"), bounds: NormalizedRect::new(0.45, 0.94, 0.1, 0.02) },
                PageTextLine { text: format!("Distinct body text {page_number}"), bounds: NormalizedRect::new(0.12, body_top, 0.7, 0.3) },
            ],
            graphics: vec![NormalizedRect::new(0.05, 0.06, 0.9, 0.002)],
        };
        let pages = vec![page(1, 0.2), page(2, 0.21), page(3, 0.19), page(4, 0.2)];
        let bands = repeating_page_bands(&pages);
        assert!(bands.iter().all(|bands| bands.header_bottom.is_some() && bands.footer_top.is_some()));
        let cropped = content_bounds(&pages[0], bands[0]).expect("body content bounds");
        assert!(cropped.top() > 0.15);
        assert!(cropped.bottom() < 0.9);
    }

    #[test]
    fn similar_text_at_different_vertical_positions_is_not_a_running_header() {
        let pages = [0.03, 0.25, 0.45]
            .into_iter()
            .map(|top| PageElements { text_lines: vec![PageTextLine { text: "Repeated chapter wording".to_owned(), bounds: NormalizedRect::new(0.1, top, 0.6, 0.03) }], graphics: Vec::new() })
            .collect::<Vec<_>>();
        assert!(repeating_page_bands(&pages).iter().all(|bands| bands.header_bottom.is_none()));
    }
}
