use crate::{Attrs, Font, FontMatchAttrs, FontVariations, HashMap, ShapeBuffer};
use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{Deref, DerefMut};
use fontdb::{FaceInfo, Query, Style, Weight};
use skrifa::raw::{ReadError, TableProvider as _};
use skrifa::{FontRef, MetadataProvider};

// re-export fontdb and harfrust
pub use fontdb;
pub use harfrust;

use super::fallback::{Fallback, Fallbacks, MonospaceFallbackInfo, PlatformFallback};

// The fields are used in the derived Ord implementation for sorting fallback candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FontMatchKey {
    pub(crate) not_emoji: bool,
    pub(crate) font_weight_diff: u16,
    pub(crate) font_stretch_diff: u16,
    pub(crate) font_style_diff: u8,
    pub(crate) font_weight: u16,
    pub(crate) font_stretch: u16,
    pub(crate) id: fontdb::ID,
    pub(crate) variable_weight_match: bool,
}

impl FontMatchKey {
    fn new(attrs: &Attrs, face: &FaceInfo, weight_axis: Option<FontWeightAxis>) -> FontMatchKey {
        // TODO: smarter way of detecting emoji
        let not_emoji = !face.post_script_name.contains("Emoji");
        let font_weight_diff = attrs.weight.0.abs_diff(face.weight.0);

        let variable_weight_match = font_weight_diff != 0
            && matches!(weight_axis, Some(FontWeightAxis::Variable { min, max })
                if (attrs.weight.0 as f32) >= min && (attrs.weight.0 as f32) <= max);
        let font_weight = face.weight.0;
        let font_stretch_diff = attrs.stretch.to_number().abs_diff(face.stretch.to_number());
        let font_stretch = face.stretch.to_number();
        let font_style_diff = match (attrs.style, face.style) {
            (Style::Normal, Style::Normal)
            | (Style::Italic, Style::Italic)
            | (Style::Oblique, Style::Oblique) => 0,
            (Style::Italic, Style::Oblique) | (Style::Oblique, Style::Italic) => 1,
            (Style::Normal, Style::Italic)
            | (Style::Normal, Style::Oblique)
            | (Style::Italic, Style::Normal)
            | (Style::Oblique, Style::Normal) => 2,
        };
        let id = face.id;
        FontMatchKey {
            not_emoji,
            font_weight_diff,
            font_stretch_diff,
            font_style_diff,
            font_weight,
            font_stretch,
            id,
            variable_weight_match,
        }
    }
}

#[derive(Clone, Copy)]
enum FontWeightAxis {
    Static,
    Variable { min: f32, max: f32 },
}

impl FontWeightAxis {
    fn read(db: &fontdb::Database, id: fontdb::ID) -> Option<Self> {
        db.with_face_data(id, |data, index| {
            let font = FontRef::from_index(data, index).ok()?;
            Some(match font.axes().get_by_tag(skrifa::Tag::new(b"wght")) {
                Some(axis) => Self::Variable { min: axis.min_value(), max: axis.max_value() },
                None => Self::Static,
            })
        })
        .flatten()
    }
}

struct CachedFont {
    font: Option<Arc<Font>>,
    // Public requests and faces used by shaped glyphs outlive fallback probing.
    retained: bool,
}

struct FontCachedCodepointSupportInfo {
    supported: Vec<u32>,
    not_supported: Vec<u32>,
}

impl FontCachedCodepointSupportInfo {
    const SUPPORTED_MAX_SZ: usize = 512;
    const NOT_SUPPORTED_MAX_SZ: usize = 1024;

    fn new() -> Self {
        Self {
            supported: Vec::with_capacity(Self::SUPPORTED_MAX_SZ),
            not_supported: Vec::with_capacity(Self::NOT_SUPPORTED_MAX_SZ),
        }
    }

    #[inline(always)]
    fn unknown_has_codepoint(
        &mut self,
        font_codepoints: &[u32],
        codepoint: u32,
        supported_insert_pos: usize,
        not_supported_insert_pos: usize,
    ) -> bool {
        let ret = font_codepoints.contains(&codepoint);
        if ret {
            // don't bother inserting if we are going to truncate the entry away
            if supported_insert_pos != Self::SUPPORTED_MAX_SZ {
                self.supported.insert(supported_insert_pos, codepoint);
                self.supported.truncate(Self::SUPPORTED_MAX_SZ);
            }
        } else {
            // don't bother inserting if we are going to truncate the entry away
            if not_supported_insert_pos != Self::NOT_SUPPORTED_MAX_SZ {
                self.not_supported
                    .insert(not_supported_insert_pos, codepoint);
                self.not_supported.truncate(Self::NOT_SUPPORTED_MAX_SZ);
            }
        }
        ret
    }

    #[inline(always)]
    fn has_codepoint(&mut self, font_codepoints: &[u32], codepoint: u32) -> bool {
        match self.supported.binary_search(&codepoint) {
            Ok(_) => true,
            Err(supported_insert_pos) => match self.not_supported.binary_search(&codepoint) {
                Ok(_) => false,
                Err(not_supported_insert_pos) => self.unknown_has_codepoint(
                    font_codepoints,
                    codepoint,
                    supported_insert_pos,
                    not_supported_insert_pos,
                ),
            },
        }
    }
}

/// Access to the system fonts.
pub struct FontSystem {
    /// The locale of the system.
    locale: String,

    /// The underlying font database.
    db: fontdb::Database,

    /// Cache for loaded fonts from the database.
    font_cache: HashMap<(fontdb::ID, Weight, FontVariations), CachedFont>,

    /// Weight axes are properties of a face, independent of the requested text style.
    font_weight_axes: HashMap<fontdb::ID, Option<FontWeightAxis>>,

    fallback_cleanup_needed: bool,

    /// Sorted unique ID's of all Monospace fonts in DB
    monospace_font_ids: Vec<fontdb::ID>,

    /// Sorted unique ID's of all Monospace fonts in DB per script.
    /// A font may support multiple scripts of course, so the same ID
    /// may appear in multiple map value vecs.
    per_script_monospace_font_ids: HashMap<[u8; 4], Vec<fontdb::ID>>,

    /// Cache for font codepoint support info
    font_codepoint_support_info_cache: HashMap<fontdb::ID, FontCachedCodepointSupportInfo>,

    /// Cache for font matches.
    font_matches_cache: HashMap<FontMatchAttrs, Arc<Vec<FontMatchKey>>>,

    family_matches_cache: HashMap<FontMatchAttrs, Arc<Vec<FontMatchKey>>>,

    /// Scratch buffer for shaping and laying out.
    pub(crate) shape_buffer: ShapeBuffer,

    /// Buffer for use in `FontFallbackIter`.
    pub(crate) monospace_fallbacks_buffer: BTreeSet<MonospaceFallbackInfo>,

    /// Cache for shaped runs
    #[cfg(feature = "shape-run-cache")]
    pub shape_run_cache: crate::ShapeRunCache,

    /// List of fallbacks
    pub(crate) dyn_fallback: Box<dyn Fallback>,

    /// List of fallbacks
    pub(crate) fallbacks: Fallbacks,
}

impl fmt::Debug for FontSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontSystem")
            .field("locale", &self.locale)
            .field("db", &self.db)
            .finish_non_exhaustive()
    }
}

impl FontSystem {
    /// Release physical pages from unused shaping buffers where supported.
    /// Buffer capacities and cached shape plans are preserved. Call at a batch
    /// boundary to keep consecutive shaping calls warm.
    pub fn release_shaping_scratch(&mut self) {
        self.shape_buffer.release_unused_pages();
    }

    const FONT_MATCHES_CACHE_SIZE_LIMIT: usize = 256;
    /// Create a new [`FontSystem`], that allows access to any installed system fonts
    ///
    /// # Timing
    ///
    /// This function takes some time to run. On the release build, it can take up to a second,
    /// while debug builds can take up to ten times longer. For this reason, it should only be
    /// called once, and the resulting [`FontSystem`] should be shared.
    pub fn new() -> Self {
        Self::new_with_fonts(core::iter::empty())
    }

    /// Create a new [`FontSystem`] with a pre-specified set of fonts.
    pub fn new_with_fonts(fonts: impl IntoIterator<Item = fontdb::Source>) -> Self {
        let locale = Self::get_locale();
        log::debug!("Locale: {locale}");

        let mut db = fontdb::Database::new();

        Self::load_fonts(&mut db, fonts.into_iter());

        //TODO: configurable default fonts
        db.set_monospace_family("Noto Sans Mono");
        db.set_sans_serif_family("Open Sans");
        db.set_serif_family("DejaVu Serif");

        Self::new_with_locale_and_db_and_fallback(locale, db, PlatformFallback)
    }

    /// Create a new [`FontSystem`] with a pre-specified locale, font database and font fallback list.
    pub fn new_with_locale_and_db_and_fallback(
        locale: String,
        db: fontdb::Database,
        impl_fallback: impl Fallback + 'static,
    ) -> Self {
        let mut monospace_font_ids = db
            .faces()
            .filter(|face_info| {
                face_info.monospaced && !face_info.post_script_name.contains("Emoji")
            })
            .map(|face_info| face_info.id)
            .collect::<Vec<_>>();
        monospace_font_ids.sort();

        let mut per_script_monospace_font_ids: HashMap<[u8; 4], BTreeSet<fontdb::ID>> =
            HashMap::default();

        if cfg!(feature = "monospace_fallback") {
            for &id in &monospace_font_ids {
                db.with_face_data(id, |font_data, face_index| {
                    let face = FontRef::from_index(font_data, face_index)?;
                    for script in face
                        .gpos()?
                        .script_list()?
                        .script_records()
                        .iter()
                        .chain(face.gsub()?.script_list()?.script_records().iter())
                    {
                        per_script_monospace_font_ids
                            .entry(script.script_tag().into_bytes())
                            .or_default()
                            .insert(id);
                    }
                    Ok::<_, ReadError>(())
                });
            }
        }

        let per_script_monospace_font_ids = per_script_monospace_font_ids
            .into_iter()
            .map(|(k, v)| (k, Vec::from_iter(v)))
            .collect();

        let fallbacks = Fallbacks::new(&impl_fallback, &[], &locale);

        Self {
            locale,
            db,
            monospace_font_ids,
            per_script_monospace_font_ids,
            font_cache: HashMap::default(),
            font_weight_axes: HashMap::default(),
            fallback_cleanup_needed: false,
            font_matches_cache: HashMap::default(),
            family_matches_cache: HashMap::default(),
            font_codepoint_support_info_cache: HashMap::default(),
            monospace_fallbacks_buffer: BTreeSet::default(),
            #[cfg(feature = "shape-run-cache")]
            shape_run_cache: crate::ShapeRunCache::default(),
            shape_buffer: ShapeBuffer::default(),
            dyn_fallback: Box::new(impl_fallback),
            fallbacks,
        }
    }

    /// Create a new [`FontSystem`] with a pre-specified locale and font database.
    pub fn new_with_locale_and_db(locale: String, db: fontdb::Database) -> Self {
        Self::new_with_locale_and_db_and_fallback(locale, db, PlatformFallback)
    }

    /// Get the locale.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Get the database.
    pub const fn db(&self) -> &fontdb::Database {
        &self.db
    }

    /// Get a mutable reference to the database.
    pub fn db_mut(&mut self) -> &mut fontdb::Database {
        // New faces can change both font selection and missing-glyph results.
        #[cfg(feature = "shape-run-cache")]
        {
            self.shape_run_cache = crate::ShapeRunCache::default();
        }
        self.font_matches_cache.clear();
        self.family_matches_cache.clear();
        self.font_weight_axes.clear();
        &mut self.db
    }

    /// Consume this [`FontSystem`] and return the locale and database.
    pub fn into_locale_and_db(self) -> (String, fontdb::Database) {
        (self.locale, self.db)
    }

    /// Get a font by its ID and weight.
    pub fn get_font(&mut self, id: fontdb::ID, weight: Weight) -> Option<Arc<Font>> {
        self.get_font_cached(id, weight, FontVariations::default(), true)
    }

    /// Resolve an exact font instance, shared with platform rasterizers.
    pub fn get_font_with_variations(
        &mut self,
        id: fontdb::ID,
        weight: Weight,
        variations: FontVariations,
    ) -> Option<Arc<Font>> {
        self.get_font_cached(id, weight, variations, true)
    }

    pub(crate) fn get_font_for_fallback(
        &mut self,
        id: fontdb::ID,
        weight: Weight,
        variations: FontVariations,
    ) -> Option<Arc<Font>> {
        self.get_font_cached(id, weight, variations, false)
    }

    fn cache_weight(&mut self, id: fontdb::ID, weight: Weight) -> Weight {
        let axis = *self
            .font_weight_axes
            .entry(id)
            .or_insert_with(|| FontWeightAxis::read(&self.db, id));
        match axis {
            Some(FontWeightAxis::Static) => self.db.face(id).map_or(weight, |face| face.weight),
            // Keep the requested weight if the face could not be inspected.
            Some(FontWeightAxis::Variable { .. }) | None => weight,
        }
    }

    fn get_font_cached(
        &mut self,
        id: fontdb::ID,
        weight: Weight,
        variations: FontVariations,
        retain: bool,
    ) -> Option<Arc<Font>> {
        let weight = self.cache_weight(id, weight);
        let entry = self
            .font_cache
            .entry((id, weight, variations))
            .or_insert_with(|| {
                #[cfg(feature = "std")]
                unsafe {
                    self.db.make_shared_face_data(id);
                }
                let font =
                    Font::new_with_variations(&self.db, id, weight, variations).map(Arc::new);
                if font.is_none() {
                    #[cfg(feature = "std")]
                    self.db.make_face_data_unshared(id);
                    log::warn!(
                        "failed to load font '{}'",
                        self.db.face(id).map_or("unknown", |face| face.post_script_name.as_str())
                    );
                }
                CachedFont { font, retained: retain }
            });
        entry.retained |= retain;
        if !entry.retained && entry.font.is_some() {
            self.fallback_cleanup_needed = true;
        }
        entry.font.clone()
    }

    pub(crate) fn retain_font(
        &mut self,
        id: fontdb::ID,
        weight: Weight,
        variations: FontVariations,
    ) {
        let weight = self.cache_weight(id, weight);
        if let Some(entry) = self.font_cache.get_mut(&(id, weight, variations)) {
            entry.retained = true;
        }
    }

    pub(crate) fn release_unused_fonts(&mut self) {
        if !core::mem::take(&mut self.fallback_cleanup_needed) {
            return;
        }
        #[cfg(feature = "std")]
        let db = &mut self.db;
        let scratch = &mut self.shape_buffer;
        let coverage = &mut self.font_codepoint_support_info_cache;
        let cleanup_needed = &mut self.fallback_cleanup_needed;
        self.font_cache.retain(|(id, _, _), entry| {
            if entry.retained || entry.font.is_none() {
                return true;
            }
            if entry.font.as_ref().is_some_and(|font| Arc::strong_count(font) > 1) {
                *cleanup_needed = true;
                return true;
            }
            #[cfg(feature = "std")]
            db.make_face_data_unshared(*id);
            scratch.release_font_plans(*id);
            coverage.remove(id);
            false
        });
    }

    pub fn is_monospace(&self, id: fontdb::ID) -> bool {
        self.monospace_font_ids.binary_search(&id).is_ok()
    }

    pub fn get_monospace_ids_for_scripts(
        &self,
        scripts: impl Iterator<Item = [u8; 4]>,
    ) -> Vec<fontdb::ID> {
        let mut ret = scripts
            .filter_map(|script| self.per_script_monospace_font_ids.get(&script))
            .flat_map(|ids| ids.iter().copied())
            .collect::<Vec<_>>();
        ret.sort();
        ret.dedup();
        ret
    }

    #[inline(always)]
    pub fn get_font_supported_codepoints_in_word(
        &mut self,
        id: fontdb::ID,
        weight: Weight,
        word: &str,
    ) -> Option<usize> {
        self.get_font_for_fallback(id, weight, FontVariations::default())
            .map(|font| {
                let code_points = font.unicode_codepoints();
                let cache = self
                    .font_codepoint_support_info_cache
                    .entry(id)
                    .or_insert_with(FontCachedCodepointSupportInfo::new);
                word.chars()
                    .filter(|ch| cache.has_codepoint(code_points, u32::from(*ch)))
                    .count()
            })
    }

    pub fn get_font_matches(&mut self, attrs: &Attrs<'_>) -> Arc<Vec<FontMatchKey>> {
        self.font_matches(attrs, false)
    }

    pub(crate) fn get_family_matches(&mut self, attrs: &Attrs<'_>) -> Arc<Vec<FontMatchKey>> {
        self.font_matches(attrs, true)
    }

    fn font_matches(&mut self, attrs: &Attrs<'_>, family_only: bool) -> Arc<Vec<FontMatchKey>> {
        let cache = if family_only { &mut self.family_matches_cache } else { &mut self.font_matches_cache };
        // Clear the cache first if it reached the size limit
        if cache.len() >= Self::FONT_MATCHES_CACHE_SIZE_LIMIT {
            log::trace!("clear font mache cache");
            cache.clear();
        }

        let db = &self.db;
        let weight_axes = &mut self.font_weight_axes;
        cache
            //TODO: do not create AttrsOwned unless entry does not already exist
            .entry(attrs.into())
            .or_insert_with(|| {
                #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
                let now = std::time::Instant::now();

                let mut font_match_keys = db
                    .faces()
                    .filter(|face| !family_only || face.families.iter().any(|family| family.0 == db.family_name(&attrs.family)))
                    .map(|face| {
                        let axis = if attrs.weight != face.weight {
                            *weight_axes.entry(face.id).or_insert_with(|| FontWeightAxis::read(db, face.id))
                        } else {
                            None
                        };
                        FontMatchKey::new(attrs, face, axis)
                    })
                    .collect::<Vec<_>>();

                // Sort so we get the keys with weight_offset=0 first
                font_match_keys.sort();

                // db.query is better than above, but returns just one font
                let query = Query {
                    families: &[attrs.family],
                    weight: attrs.weight,
                    stretch: attrs.stretch,
                    style: attrs.style,
                };

                if let Some(id) = db.query(&query) {
                    if let Some(i) = font_match_keys
                        .iter()
                        .enumerate()
                        .find(|(_i, key)| key.id == id)
                        .map(|(i, _)| i)
                    {
                        // if exists move to front
                        let match_key = font_match_keys.remove(i);
                        font_match_keys.insert(0, match_key);
                    } else if let Some(face) = db.face(id) {
                        // else insert in front
                        let axis = *weight_axes.entry(id).or_insert_with(|| FontWeightAxis::read(db, id));
                        let match_key = FontMatchKey::new(attrs, face, axis);
                        font_match_keys.insert(0, match_key);
                    } else {
                        log::error!("Could not get face from db, that should've been there.");
                    }
                }

                #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
                {
                    let elapsed = now.elapsed();
                    log::debug!("font matches for {attrs:?} in {elapsed:?}");
                }

                Arc::new(font_match_keys)
            })
            .clone()
    }

    #[cfg(feature = "std")]
    fn get_locale() -> String {
        sys_locale::get_locale().unwrap_or_else(|| {
            log::warn!("failed to get system locale, falling back to en-US");
            String::from("en-US")
        })
    }

    #[cfg(not(feature = "std"))]
    fn get_locale() -> String {
        String::from("en-US")
    }

    #[cfg(feature = "std")]
    fn load_fonts(db: &mut fontdb::Database, fonts: impl Iterator<Item = fontdb::Source>) {
        #[cfg(not(target_arch = "wasm32"))]
        let now = std::time::Instant::now();

        db.load_system_fonts();

        for source in fonts {
            db.load_font_source(source);
        }

        #[cfg(not(target_arch = "wasm32"))]
        log::debug!(
            "Parsed {} font faces in {}ms.",
            db.len(),
            now.elapsed().as_millis()
        );
    }

    #[cfg(not(feature = "std"))]
    fn load_fonts(db: &mut fontdb::Database, fonts: impl Iterator<Item = fontdb::Source>) {
        for source in fonts {
            db.load_font_source(source);
        }
    }
}

/// A value borrowed together with an [`FontSystem`]
#[derive(Debug)]
pub struct BorrowedWithFontSystem<'a, T> {
    pub(crate) inner: &'a mut T,
    pub(crate) font_system: &'a mut FontSystem,
}

impl<T> Deref for BorrowedWithFontSystem<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<T> DerefMut for BorrowedWithFontSystem<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}
