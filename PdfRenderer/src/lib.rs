//! Framework-independent PDF sessions, rendering, searching, selection, and page caching.

use pdfium_render::prelude::*;
pub use reader_style::{ReaderInteractionStyle, RgbaColor};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const PDFIUM_BYTES: &[u8] = include_bytes!(env!("PDFIUM_EMBEDDED_PATH"));
const PDFIUM_LIBRARY_NAME: &str = env!("PDFIUM_LIBRARY_NAME");
const PDFIUM_THIRD_PARTY_NOTICES: &str = include_str!(env!("PDFIUM_NOTICES_PATH"));
const PDFIUM_VERSION: &str = "7961";
const DEFAULT_RENDER_WIDTH: u16 = 1400;
const MIN_RENDER_WIDTH: u16 = 320;
const MAX_RENDER_WIDTH: u16 = 3200;

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

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

impl PdfDocumentSession {
    pub fn open(path: impl Into<PathBuf>) -> PdfResult<Self> {
        let path = path.into();
        let bytes = fs::read(&path).map_err(|error| PdfError::new(format!("reading {}: {error}", path.display())))?;
        Self::from_bytes(path, bytes)
    }

    pub fn from_bytes(display_path: impl Into<PathBuf>, bytes: Vec<u8>) -> PdfResult<Self> {
        let path = display_path.into();
        let document = shared_pdfium()?.load_pdf_from_byte_vec(bytes, None).map_err(|error| PdfError::new(format!("opening {}: {error}", path.display())))?;
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
        let page = self.document.pages().get(page_index as i32).map_err(|_| PdfError::new(format!("PDF page {} does not exist", page_index + 1)))?;
        let width = target_width.unwrap_or(DEFAULT_RENDER_WIDTH).clamp(MIN_RENDER_WIDTH, MAX_RENDER_WIDTH) as i32;
        let text_layout = extract_text_layout(&page, page_index)?;
        let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(width).render_annotations(true)).map_err(|error| PdfError::new(format!("rendering PDF page {}: {error}", page_index + 1)))?;
        let pixel_width = bitmap.width() as u32;
        let pixel_height = bitmap.height() as u32;
        Ok(RenderedPdfPage { rgba: bitmap.as_rgba_bytes(), pixel_width, pixel_height, text_layout: Arc::new(text_layout) })
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NormalizedRect {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

impl NormalizedRect {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        let left = left.clamp(0.0, 1.0);
        let top = top.clamp(0.0, 1.0);
        Self { left, top, width: width.clamp(0.0, 1.0 - left), height: height.clamp(0.0, 1.0 - top) }
    }

    pub fn left(&self) -> f32 {
        self.left
    }

    pub fn top(&self) -> f32 {
        self.top
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }
}

#[derive(Clone, Debug)]
pub struct PdfTextSelection {
    text: Arc<str>,
    rects: Arc<[NormalizedRect]>,
}

impl PdfTextSelection {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn rects(&self) -> &[NormalizedRect] {
        &self.rects
    }
}

#[derive(Clone, Debug)]
pub struct RenderedPdfPage {
    rgba: Vec<u8>,
    pixel_width: u32,
    pixel_height: u32,
    text_layout: Arc<PdfPageTextLayout>,
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

    pub fn select_text(&self, start: (f32, f32), end: (f32, f32)) -> Option<PdfTextSelection> {
        self.text_layout.select(start, end)
    }

    pub fn into_parts(self) -> (Vec<u8>, u32, u32, Arc<PdfPageTextLayout>) {
        (self.rgba, self.pixel_width, self.pixel_height, self.text_layout)
    }
}

#[derive(Clone, Debug)]
pub struct PdfPageTextLayout {
    page_index: usize,
    characters: Vec<PdfTextCharacter>,
}

impl Default for PdfPageTextLayout {
    fn default() -> Self {
        Self { page_index: 0, characters: Vec::new() }
    }
}

impl PdfPageTextLayout {
    pub fn select(&self, start: (f32, f32), end: (f32, f32)) -> Option<PdfTextSelection> {
        let start_index = nearest_character(&self.characters, start, true)?;
        let end_index = nearest_character(&self.characters, end, false)?;
        let (first, last) = if start_index <= end_index { (start_index, end_index) } else { (end_index, start_index) };
        let selected = self.characters.iter().filter(|character| character.index >= first && character.index <= last).collect::<Vec<_>>();
        let text = selected.iter().map(|character| character.character).collect::<String>();
        let rects = merge_selection_lines(selected.iter().filter(|character| !character.character.is_control()).map(|character| character.bounds));
        if text.is_empty() || rects.is_empty() {
            return None;
        }
        let _ = self.page_index;
        Some(PdfTextSelection { text: text.into(), rects: rects.into() })
    }
}

#[derive(Clone, Copy, Debug)]
struct PdfTextCharacter {
    index: usize,
    character: char,
    bounds: NormalizedRect,
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
}

impl PageRenderKey {
    pub fn new(page_index: usize, target_width: u16) -> Self {
        Self { page_index, target_width: target_width.clamp(MIN_RENDER_WIDTH, MAX_RENDER_WIDTH) }
    }

    pub fn page_index(&self) -> usize {
        self.page_index
    }

    pub fn target_width(&self) -> u16 {
        self.target_width
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
            let bounds = character.loose_bounds().or_else(|_| character.tight_bounds()).ok()?;
            Some(PdfTextCharacter { index: character.index(), character: unicode, bounds: normalized_pdf_bounds(bounds, width, height) })
        })
        .collect();
    Ok(PdfPageTextLayout { page_index, characters })
}

fn normalized_pdf_bounds(bounds: PdfRect, page_width: f32, page_height: f32) -> NormalizedRect {
    NormalizedRect::new(bounds.left().value / page_width, 1.0 - bounds.top().value / page_height, bounds.width().value / page_width, bounds.height().value / page_height)
}

fn nearest_character(characters: &[PdfTextCharacter], point: (f32, f32), require_near: bool) -> Option<usize> {
    characters
        .iter()
        .filter(|character| !character.character.is_control())
        .map(|character| (character.index, distance_to_rect_squared(point, character.bounds)))
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .filter(|(_, distance)| !require_near || *distance <= 0.0016)
        .map(|(index, _)| index)
}

fn distance_to_rect_squared(point: (f32, f32), rect: NormalizedRect) -> f32 {
    let right = rect.left + rect.width;
    let bottom = rect.top + rect.height;
    let dx = if point.0 < rect.left {
        rect.left - point.0
    } else if point.0 > right {
        point.0 - right
    } else {
        0.0
    };
    let dy = if point.1 < rect.top {
        rect.top - point.1
    } else if point.1 > bottom {
        point.1 - bottom
    } else {
        0.0
    };
    dx * dx + dy * dy
}

fn merge_selection_lines(rects: impl Iterator<Item = NormalizedRect>) -> Vec<NormalizedRect> {
    rects.fold(Vec::new(), |mut lines, rect| {
        if let Some(last) = lines.last_mut()
            && same_text_line(*last, rect)
        {
            *last = union_rect(*last, rect);
        } else {
            lines.push(rect);
        }
        lines
    })
}

fn same_text_line(left: NormalizedRect, right: NormalizedRect) -> bool {
    let overlap = (left.top + left.height).min(right.top + right.height) - left.top.max(right.top);
    overlap > left.height.min(right.height) * 0.3
}

fn union_rect(left: NormalizedRect, right: NormalizedRect) -> NormalizedRect {
    let x = left.left.min(right.left);
    let y = left.top.min(right.top);
    NormalizedRect::new(x, y, (left.left + left.width).max(right.left + right.width) - x, (left.top + left.height).max(right.top + right.height) - y)
}

fn shared_pdfium() -> PdfResult<&'static Pdfium> {
    let pdfium = PDFIUM.get_or_init(|| {
        let path = extracted_pdfium_path().map_err(|error| error.to_string())?;
        let bindings = Pdfium::bind_to_library(&path).map_err(|error| format!("loading embedded PDFium from {}: {error}", path.display()))?;
        Ok(Pdfium::new(bindings))
    });
    pdfium.as_ref().map_err(|error| PdfError::new(error.clone()))
}

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
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn fake_page(byte_len: usize) -> Arc<RenderedPdfPage> {
        Arc::new(RenderedPdfPage { rgba: vec![0; byte_len], pixel_width: 1, pixel_height: 1, text_layout: Arc::new(PdfPageTextLayout { page_index: 0, characters: Vec::new() }) })
    }

    #[test]
    fn page_render_keys_bound_requested_width() {
        assert_eq!(PageRenderKey::new(2, 1).target_width(), MIN_RENDER_WIDTH);
        assert_eq!(PageRenderKey::new(2, u16::MAX).target_width(), MAX_RENDER_WIDTH);
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
        let layout = PdfPageTextLayout {
            page_index: 0,
            characters: vec![PdfTextCharacter { index: 0, character: 'A', bounds: NormalizedRect::new(0.1, 0.2, 0.04, 0.03) }, PdfTextCharacter { index: 1, character: 'B', bounds: NormalizedRect::new(0.14, 0.2, 0.04, 0.03) }],
        };
        let selection = layout.select((0.11, 0.21), (0.17, 0.21)).expect("text selection");
        assert_eq!(selection.text(), "AB");
        assert_eq!(selection.rects().len(), 1);
        assert!((selection.rects()[0].width() - 0.08).abs() < 0.0001);
    }

    #[test]
    fn embedded_pdfium_session_opens_once_renders_searches_and_selects() -> PdfResult<()> {
        let path = fixture_path("reader")?;
        create_fixture_pdf(&path)?;
        let session = PdfDocumentSession::open(&path)?;
        assert_eq!(session.info().page_count(), 2);
        assert!(session.info().pages().iter().all(|page| page.aspect_ratio() > 0.0));
        let rendered = session.render_page(1, Some(640))?;
        assert_eq!(rendered.pixel_width(), 640);
        assert!(rendered.pixel_height() > rendered.pixel_width());
        assert_eq!(rendered.rgba().len(), rendered.pixel_width() as usize * rendered.pixel_height() as usize * 4);
        let hits = session.search("Second searchable")?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_index(), 1);
        let hit = hits[0].bounds();
        let selection = rendered.select_text((hit.left() + 0.001, hit.top() + hit.height() * 0.5), (hit.left() + hit.width(), hit.top() + hit.height() * 0.5)).expect("selection from search bounds");
        assert!(selection.text().contains("Second searchable"));
        drop(session);
        fs::remove_file(path).map_err(|error| PdfError::new(error.to_string()))?;
        Ok(())
    }
}
