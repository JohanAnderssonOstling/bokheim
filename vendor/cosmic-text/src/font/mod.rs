// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::FontVariations;
use harfrust::Shaper;
use linebender_resource_handle::{Blob, FontData};
use skrifa::raw::{ReadError, TableProvider as _};
use skrifa::{metrics::Metrics, prelude::*};
// re-export skrifa
pub use skrifa;
// re-export peniko::Font;
#[cfg(feature = "peniko")]
pub use linebender_resource_handle::FontData as PenikoFont;

use core::fmt;

use alloc::sync::Arc;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
use fontdb::Style;
use self_cell::self_cell;

pub mod fallback;
pub use fallback::{Fallback, PlatformFallback};

pub use self::system::*;
mod system;

struct OwnedFaceData {
    data: Arc<dyn AsRef<[u8]> + Send + Sync>,
    shaper_data: harfrust::ShaperData,
    shaper_instance: harfrust::ShaperInstance,
    metrics: Metrics,
}

self_cell!(
    struct OwnedFace {
        owner: OwnedFaceData,

        #[covariant]
        dependent: Shaper,
    }
);

struct FontMonospaceFallback {
    monospace_em_width: Option<f32>,
    scripts: Vec<[u8; 4]>,
    unicode_codepoints: Vec<u32>,
}

/// A font
pub struct Font {
    #[cfg(feature = "swash")]
    swash: (u32, swash::CacheKey),
    harfrust: OwnedFace,
    data: FontData,
    normalized_coords: Vec<i16>,
    oblique_angle: f32,
    has_slant_axis: bool,
    italic_instance: bool,
    id: fontdb::ID,
    monospace_fallback: Option<FontMonospaceFallback>,
    pub(crate) italic_or_oblique: bool,
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Font {
    pub const fn id(&self) -> fontdb::ID {
        self.id
    }

    pub fn monospace_em_width(&self) -> Option<f32> {
        self.monospace_fallback
            .as_ref()
            .and_then(|x| x.monospace_em_width)
    }

    pub fn scripts(&self) -> &[[u8; 4]] {
        self.monospace_fallback.as_ref().map_or(&[], |x| &x.scripts)
    }

    pub fn unicode_codepoints(&self) -> &[u32] {
        self.monospace_fallback
            .as_ref()
            .map_or(&[], |x| &x.unicode_codepoints)
    }

    pub fn data(&self) -> &[u8] {
        self.data.data.data()
    }

    pub fn shaper(&self) -> &harfrust::Shaper<'_> {
        self.harfrust.borrow_dependent()
    }

    pub(crate) fn shaper_instance(&self) -> &harfrust::ShaperInstance {
        &self.harfrust.borrow_owner().shaper_instance
    }

    /// Coordinates shared by shaping, metrics and rasterization.
    pub fn normalized_coords(&self) -> &[i16] {
        &self.normalized_coords
    }

    /// CSS angle represented by the selected slant axis or static font face.
    pub fn oblique_angle(&self) -> f32 {
        self.oblique_angle
    }

    /// Whether the font supplies its own variable slant outlines.
    pub fn is_italic_instance(&self) -> bool { self.italic_instance }

    pub fn has_slant_axis(&self) -> bool {
        self.has_slant_axis
    }

    pub fn metrics(&self) -> &Metrics {
        &self.harfrust.borrow_owner().metrics
    }

    #[cfg(feature = "peniko")]
    pub fn as_peniko(&self) -> PenikoFont {
        self.data.clone()
    }

    #[cfg(feature = "swash")]
    pub fn as_swash(&self) -> swash::FontRef<'_> {
        let swash = &self.swash;
        swash::FontRef {
            data: self.data(),
            offset: swash.0,
            key: swash.1,
        }
    }
}

impl Font {
    pub fn new(db: &fontdb::Database, id: fontdb::ID, weight: fontdb::Weight) -> Option<Self> {
        Self::new_with_variations(db, id, weight, FontVariations::default())
    }

    pub fn new_with_variations(
        db: &fontdb::Database,
        id: fontdb::ID,
        weight: fontdb::Weight,
        variations: FontVariations,
    ) -> Option<Self> {
        let info = db.face(id)?;

        let data = match &info.source {
            fontdb::Source::Binary(data) => Arc::clone(data),
            #[cfg(feature = "std")]
            fontdb::Source::File(path) => {
                log::warn!("Unsupported fontdb Source::File('{}')", path.display());
                return None;
            }
            #[cfg(feature = "std")]
            fontdb::Source::SharedFile(_path, data) => Arc::clone(data),
        };

        // It's a bit unfortunate but we need to parse the data into a `FontRef`
        // twice--once to construct the HarfRust `ShaperInstance` and
        // `ShaperData`, and once to create the persistent `FontRef` tied to the
        // lifetime of the face data.
        let font_ref = FontRef::from_index((*data).as_ref(), info.index).ok()?;
        let slant_axis = font_ref.axes().get_by_tag(Tag::new(b"slnt"));
        let italic_axis = font_ref.axes().get_by_tag(Tag::new(b"ital"));
        // Italic falls back to the standard oblique angle when only `slnt`
        // exists. An `ital` axis supplies its own italic design instead.
        let fallback_slant = if italic_axis.is_none()
            && variations
                .settings()
                .any(|(tag, value)| tag == *b"ital" && value == 1.0)
            && variations.slant().is_none()
        {
            slant_axis.as_ref().map(|_| -14.0)
        } else {
            None
        };
        let oblique_angle = slant_axis.as_ref().map_or_else(
            || {
                font_ref
                    .post()
                    .map(|post| -(post.italic_angle().to_f64() as f32))
                    .unwrap_or(0.0)
            },
            |axis| {
                -variations
                    .slant()
                    .or(fallback_slant)
                    .unwrap_or(axis.default_value())
                    .clamp(axis.min_value(), axis.max_value())
            },
        );
        let has_slant_axis = slant_axis.is_some();
        let italic_instance = info.style == Style::Italic || (italic_axis.is_some()
            && variations.settings().any(|(tag, value)| tag == *b"ital" && value > 0.0));
        let location = font_ref.axes().location(
            core::iter::once((Tag::new(b"wght"), weight.0 as f32))
                .chain(
                    variations
                        .settings()
                        .map(|(tag, value)| (Tag::new(&tag), value)),
                )
                .chain(fallback_slant.map(|slant| (Tag::new(b"slnt"), slant))),
        );
        let normalized_coords = location
            .coords()
            .iter()
            .map(|coord| coord.to_bits())
            .collect();
        let metrics = font_ref.metrics(Size::unscaled(), &location);

        let monospace_fallback = if cfg!(feature = "monospace_fallback") {
            (|| {
                let glyph_metrics = font_ref.glyph_metrics(Size::unscaled(), &location);
                let charmap = font_ref.charmap();
                let monospace_em_width = info
                    .monospaced
                    .then(|| {
                        let hor_advance = glyph_metrics.advance_width(charmap.map(' ')?)?;
                        let upem = metrics.units_per_em;
                        Some(hor_advance / f32::from(upem))
                    })
                    .flatten();

                if info.monospaced && monospace_em_width.is_none() {
                    None?;
                }

                let scripts = font_ref
                    .gpos()
                    .ok()?
                    .script_list()
                    .ok()?
                    .script_records()
                    .iter()
                    .chain(
                        font_ref
                            .gsub()
                            .ok()?
                            .script_list()
                            .ok()?
                            .script_records()
                            .iter(),
                    )
                    .map(|script| script.script_tag().into_bytes())
                    .collect();

                let mut unicode_codepoints = Vec::new();

                for (code_point, _) in charmap.mappings() {
                    unicode_codepoints.push(code_point);
                }

                unicode_codepoints.shrink_to_fit();

                Some(FontMonospaceFallback {
                    monospace_em_width,
                    scripts,
                    unicode_codepoints,
                })
            })()
        } else {
            None
        };

        let (shaper_instance, shaper_data) = {
            (
                harfrust::ShaperInstance::from_coords(&font_ref, location.coords().iter().copied()),
                harfrust::ShaperData::new(&font_ref),
            )
        };

        Some(Self {
            id: info.id,
            normalized_coords,
            oblique_angle,
            has_slant_axis,
            italic_instance,
            monospace_fallback,
            #[cfg(feature = "swash")]
            swash: {
                let swash = swash::FontRef::from_index((*data).as_ref(), info.index as usize)?;
                (swash.offset, swash.key)
            },
            harfrust: OwnedFace::try_new(
                OwnedFaceData {
                    data: Arc::clone(&data),
                    shaper_data,
                    shaper_instance,
                    metrics,
                },
                |OwnedFaceData {
                     data,
                     shaper_data,
                     shaper_instance,
                     ..
                 }| {
                    let font_ref = FontRef::from_index((**data).as_ref(), info.index)?;
                    let shaper = shaper_data
                        .shaper(&font_ref)
                        .instance(Some(shaper_instance))
                        .build();
                    Ok::<_, ReadError>(shaper)
                },
            )
            .ok()?,
            data: FontData::new(Blob::new(data), info.index),
            italic_or_oblique: info.style == Style::Italic || info.style == Style::Oblique,
        })
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_fonts_load_time() {
        use crate::FontSystem;
        use sys_locale::get_locale;

        #[cfg(not(target_arch = "wasm32"))]
        let now = std::time::Instant::now();

        let mut db = fontdb::Database::new();
        let locale = get_locale().expect("Local available");
        db.load_system_fonts();
        FontSystem::new_with_locale_and_db(locale, db);

        #[cfg(not(target_arch = "wasm32"))]
        println!("Fonts load time {}ms.", now.elapsed().as_millis());
    }
}
