//! Publication font loading, decoding and persistent remote caching. No UI code.
//!
//! Native readers store successful HTTP(S) downloads as validated SFNT files in
//! `<app-data>/cache/fonts/v1`, configured before readers are constructed. URL
//! hashes identify entries; OS locks deduplicate concurrent requests and atomic
//! writes prevent partial entries. Cached fonts are reused without HTTP checks
//! until the cache is deleted. Failed downloads are never cached as fonts.
//!
//! Web readers use the persistent `bokheim-fonts-v1` browser CacheStorage cache
//! with in-flight request sharing. Local publication resources and data URLs are
//! decoded directly. Sources are tried in authored order, with unsupported
//! formats/technologies skipped. The GPUI adapter registers the returned private
//! family and asks the reader to refresh shaping when loading completes.

#[cfg(not(target_arch = "wasm32"))]
mod disk;
mod sfnt;
#[cfg(target_arch = "wasm32")]
mod web;

use html::resources::ResourceProvider;
use html::style::{FontFace, FontFaceSource};
use html::text::FontSlant;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
pub use disk::{FontDiskCache, configure_cache_directory};

pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;

/// A face with a private, content-addressed family name, preventing publication
/// aliases or font-internal names from colliding in the platform's global DB.
#[derive(Clone, Debug)]
pub struct LoadedFontFace {
    pub declaration: FontFace,
    pub family: String,
    pub bytes: Option<Arc<[u8]>>,
    pub weight: u16,
    pub slant: FontSlant,
    pub variable: bool,
}

/// Stable within a publication, including separate Unicode subsets and faces.
pub fn face_key(face: &FontFace) -> String {
    blake3::hash(format!("{face:?}").as_bytes()).to_hex().to_string()
}

/// Called on a host background executor on native platforms. Tries alternatives
/// in order and only caches successful, decoded font data.
pub async fn load_font_face(face: FontFace, provider: Arc<dyn ResourceProvider>, installed: &[String]) -> Result<LoadedFontFace, String> {
    let mut errors = Vec::new();
    for source in &face.sources {
        match source {
            FontFaceSource::Local(name) => {
                if let Some(family) = installed.iter().find(|family| family.eq_ignore_ascii_case(name)) {
                    return Ok(LoadedFontFace { declaration: face.clone(), family: family.clone(), bytes: None, weight: 400, slant: FontSlant::Normal, variable: true });
                }
            }
            FontFaceSource::Url { url, format, technologies } => {
                if !supported_source(format.as_deref(), technologies) {
                    continue;
                }
                let result = load_url(url, provider.as_ref()).await.and_then(|bytes| sfnt::prepare(&bytes, &face));
                match result {
                    Ok(loaded) => return Ok(loaded),
                    Err(error) => errors.push(format!("{url}: {error}")),
                }
            }
        }
    }
    Err(format!("no usable source for font {}: {}", face.family, errors.join("; ")))
}

fn supported_source(format: Option<&str>, technologies: &[String]) -> bool {
    format.is_none_or(|format| matches!(format.to_ascii_lowercase().as_str(), "woff" | "woff2" | "truetype" | "opentype" | "collection" | "woff-variations" | "woff2-variations" | "truetype-variations" | "opentype-variations"))
        && technologies.iter().all(|technology| matches!(technology.as_str(), "features-opentype" | "variations"))
}

async fn load_url(url: &str, provider: &dyn ResourceProvider) -> Result<Vec<u8>, String> {
    let resource = url.split('#').next().unwrap_or(url);
    if resource.starts_with("data:") {
        let data = data_url::DataUrl::process(resource).map_err(|error| format!("invalid data URL: {error:?}"))?;
        let (bytes, _) = data.decode_to_vec().map_err(|error| format!("invalid font data: {error:?}"))?;
        return decode(&bytes);
    }
    if resource.starts_with("https://") || resource.starts_with("http://") {
        #[cfg(not(target_arch = "wasm32"))]
        return FontDiskCache::configured()?.load(resource);
        #[cfg(target_arch = "wasm32")]
        return web::load(resource).await;
    }
    let bytes = provider.read_bytes_async(resource).await.map_err(|error| error.to_string())?;
    decode(&bytes)
}

/// Decode web fonts to SFNT before validation or persistence. Corrupt responses
/// and HTML error pages must never become successful cache entries.
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_FONT_BYTES {
        return Err("font exceeds the size limit".into());
    }
    let signature = bytes.get(..4).ok_or("truncated font")?;
    let decoded = match signature {
        b"wOFF" | b"wOF2" => {
            let size = bytes.get(16..20).and_then(|value| <[u8; 4]>::try_from(value).ok()).map(u32::from_be_bytes).ok_or("truncated web font header")?;
            if size as usize > MAX_FONT_BYTES {
                return Err("decoded font exceeds the size limit".into());
            }
            if signature == b"wOFF" { wuff::decompress_woff1(bytes) } else { wuff::decompress_woff2(bytes) }.map_err(|error| format!("web font decoding failed: {error:?}"))?
        }
        _ => bytes.to_vec(),
    };
    if decoded.len() > MAX_FONT_BYTES {
        return Err("decoded font exceeds the size limit".into());
    }
    ttf_parser::Face::parse(&decoded, 0).map_err(|error| format!("invalid font: {error:?}"))?;
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use html::style::FontFaceStyle;
    use std::future::Future;
    use std::io;
    use std::pin::Pin;
    use std::sync::Mutex;

    struct Provider {
        reads: Mutex<Vec<String>>,
    }
    impl ResourceProvider for Provider {
        fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
            self.reads.lock().unwrap().push(uri.into());
            if uri == "working.ttf" { Ok(font().to_vec()) } else { Err(io::ErrorKind::NotFound.into()) }
        }
        fn read_bytes_async<'a>(&'a self, uri: &'a str) -> Pin<Box<dyn Future<Output = io::Result<Vec<u8>>> + Send + 'a>> {
            Box::pin(async move {
                if uri == "deferred.ttf" {
                    self.reads.lock().unwrap().push(uri.into());
                    Ok(font().to_vec())
                } else {
                    self.read_bytes(uri)
                }
            })
        }
        fn exists(&self, _: &str) -> bool {
            false
        }
        fn resolve(&self, _: &str, href: &str) -> String {
            href.into()
        }
        fn list_html_candidates(&self, _: &str) -> io::Result<Vec<String>> {
            Ok(vec![])
        }
    }
    fn font() -> &'static [u8] {
        include_bytes!("../../../apps/desktop-gpui/assets/fonts/NV_Garamond-Regular.ttf")
    }
    fn face(sources: Vec<FontFaceSource>) -> FontFace {
        FontFace { family: "CSS Alias".into(), sources, weight: 400.0..=400.0, style: FontFaceStyle::Normal, stretch: 100.0..=100.0, unicode_ranges: vec![0..=0x10ffff], additional_descriptors: vec![] }
    }
    fn url(url: &str) -> FontFaceSource {
        FontFaceSource::Url { url: url.into(), format: None, technologies: vec![] }
    }

    #[test]
    fn woff2_fonts_decode_and_keep_their_glyphs() {
        // NV Garamond subset to "AB ", renamed BokheimFontTest, then encoded
        // with fontTools.subset/TTFont.flavor = "woff2". OFL in fixtures.
        let bytes = decode(include_bytes!("../tests/fixtures/reader-test.woff2")).unwrap();
        let parsed = ttf_parser::Face::parse(&bytes, 0).unwrap();
        assert!(parsed.glyph_index('A').is_some());
        assert!(parsed.glyph_index('B').is_some());
        assert!(parsed.glyph_index('Z').is_none());
    }

    #[test]
    fn ordered_sources_fall_back_after_missing_and_unsupported_fonts() {
        let provider = Arc::new(Provider { reads: Mutex::default() });
        let sources = vec![FontFaceSource::Local("Unavailable".into()), url("missing.ttf"), FontFaceSource::Url { url: "unsupported.bin".into(), format: Some("future-format".into()), technologies: vec![] }, url("working.ttf")];
        let loaded = futures_lite::future::block_on(load_font_face(face(sources), provider.clone(), &[])).unwrap();
        assert_eq!(provider.reads.lock().unwrap().as_slice(), ["missing.ttf", "working.ttf"]);
        assert_eq!(loaded.declaration.family, "CSS Alias");
        assert!(loaded.family.starts_with("BokheimFace"));
    }

    #[test]
    fn publication_fonts_can_load_outside_the_prepared_text_snapshot() {
        let provider = Arc::new(Provider { reads: Mutex::default() });
        let loaded = futures_lite::future::block_on(load_font_face(face(vec![url("deferred.ttf")]), provider.clone(), &[])).unwrap();
        assert!(loaded.family.starts_with("BokheimFace"));
        assert!(loaded.bytes.is_some());
        assert_eq!(provider.reads.lock().unwrap().as_slice(), ["deferred.ttf"]);
    }

    #[test]
    fn installed_local_source_prevents_loading_later_urls() {
        let provider = Arc::new(Provider { reads: Mutex::default() });
        let loaded = futures_lite::future::block_on(load_font_face(face(vec![FontFaceSource::Local("Installed".into()), url("working.ttf")]), provider.clone(), &["Installed".into()])).unwrap();
        assert_eq!(loaded.family, "Installed");
        assert!(loaded.bytes.is_none());
        assert!(provider.reads.lock().unwrap().is_empty());
    }

    #[test]
    fn data_font_urls_decode_without_provider_reads() {
        let provider = Arc::new(Provider { reads: Mutex::default() });
        let data = format!("data:font/ttf;base64,{}", STANDARD.encode(font()));
        let loaded = futures_lite::future::block_on(load_font_face(face(vec![url(&data)]), provider.clone(), &[])).unwrap();
        assert!(loaded.bytes.is_some());
        assert!(provider.reads.lock().unwrap().is_empty());
    }

    #[test]
    fn woff_decoding_preserves_font_geometry() {
        // Construct a standards-compliant WOFF1 with uncompressed tables from
        // the bundled font. This exercises the actual decoder with real data.
        let original = ttf_parser::Face::parse(font(), 0).unwrap();
        let records = original.raw_face().table_records;
        let mut woff = vec![0; 44 + usize::from(records.len()) * 20];
        woff[..4].copy_from_slice(b"wOFF");
        woff[4..8].copy_from_slice(&[0, 1, 0, 0]);
        woff[12..14].copy_from_slice(&records.len().to_be_bytes());
        woff[16..20].copy_from_slice(&(font().len() as u32).to_be_bytes());
        for (index, record) in records.into_iter().enumerate() {
            let offset = woff.len();
            let begin = record.offset as usize;
            let table = &font()[begin..begin + record.length as usize];
            let mut directory = record.tag.to_bytes().to_vec();
            for value in [offset as u32, record.length, record.length, record.check_sum] {
                directory.extend(value.to_be_bytes());
            }
            woff[44 + index * 20..64 + index * 20].copy_from_slice(&directory);
            woff.extend(table);
            woff.resize(woff.len().next_multiple_of(4), 0);
        }
        let length = woff.len() as u32;
        woff[8..12].copy_from_slice(&length.to_be_bytes());
        let decoded = decode(&woff).unwrap();
        let decoded = ttf_parser::Face::parse(&decoded, 0).unwrap();
        assert_eq!(decoded.glyph_index('A'), original.glyph_index('A'));
        assert_eq!(decoded.units_per_em(), original.units_per_em());
    }

    #[test]
    fn invalid_fonts_and_oversized_web_font_headers_are_rejected() {
        assert!(decode(b"<html>Error</html>").is_err());
        let mut header = vec![0; 48];
        header[..4].copy_from_slice(b"wOF2");
        header[16..20].copy_from_slice(&((MAX_FONT_BYTES + 1) as u32).to_be_bytes());
        assert!(decode(&header).unwrap_err().contains("size limit"));
    }
}
