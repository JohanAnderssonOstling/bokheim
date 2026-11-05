//! The retained master and versioned card derivatives.
use std::{
    fs,
    io::Cursor,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use thumbnail::{BROWSE_THUMBNAIL_WIDTH, THUMBNAIL_WIDTH, ThumbnailVersions};

#[derive(Clone, Copy)]
enum Variant {
    Master,
    Card(u32),
}

const CARD_WIDTHS: [u32; 2] = [BROWSE_THUMBNAIL_WIDTH, THUMBNAIL_WIDTH];
const VARIANTS: [Variant; 3] = [Variant::Master, Variant::Card(CARD_WIDTHS[0]), Variant::Card(CARD_WIDTHS[1])];

pub(crate) struct CoverAssets<'a> {
    directory: &'a Path,
    key: &'a str,
}

impl<'a> CoverAssets<'a> {
    pub fn new(directory: &'a Path, key: &'a str) -> Self {
        Self { directory, key }
    }

    fn path(&self, variant: Variant) -> PathBuf {
        let suffix = match variant {
            Variant::Master => "detail".to_owned(),
            Variant::Card(width) => thumbnail::card_thumbnail_suffix(width),
        };
        self.directory.join("covers").join(format!("{}-{suffix}.jpg", self.key))
    }

    /// Both the browse card and retained master are required.
    pub fn exists(&self) -> bool {
        self.has_browse() && self.path(Variant::Master).is_file()
    }

    pub fn has_browse(&self) -> bool {
        self.path(Variant::Card(BROWSE_THUMBNAIL_WIDTH)).is_file()
    }

    pub fn store(&self, versions: ThumbnailVersions) -> Result<(), String> {
        for (variant, bytes) in [(Variant::Card(BROWSE_THUMBNAIL_WIDTH), versions.browse.as_slice()), (Variant::Card(THUMBNAIL_WIDTH), versions.card_high_density.as_slice()), (Variant::Master, versions.high_density.as_slice())] {
            fs::write(self.path(variant), bytes).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn store_image(&self, bytes: &[u8]) -> Result<(), String> {
        let format = image::guess_format(bytes).map_err(|error| error.to_string())?;
        let decoded = thumbnail::decode_cover_image(bytes, format).map_err(|error| error.to_string())?;
        self.store(ThumbnailVersions::from_image(&decoded).map_err(|error| error.to_string())?)
    }

    pub fn extract_book(&self, bytes: &[u8]) -> Result<bool, String> {
        let versions = thumbnail::generate_thumbnail_versions_with_pdf_renderer("epub", Cursor::new(bytes.to_vec()), thumbnail::render_pdf_cover).map_err(|error| error.to_string())?;
        let Some(versions) = versions else { return Ok(false) };
        self.store(versions)?;
        Ok(true)
    }

    /// Rebuild only missing derivatives, independently from the retained
    /// master. Existing cards and the selected source artwork stay intact.
    pub fn generate_cards(&self) -> Result<(), String> {
        let source = self.path(Variant::Master);
        if !source.exists() || CARD_WIDTHS.into_iter().all(|width| self.path(Variant::Card(width)).exists()) {
            return Ok(());
        }
        let bytes = fs::read(source).map_err(|error| error.to_string())?;
        let format = image::guess_format(&bytes).map_err(|error| error.to_string())?;
        let image = thumbnail::decode_cover_image(&bytes, format).map_err(|error| error.to_string())?;
        for width in CARD_WIDTHS {
            let path = self.path(Variant::Card(width));
            if !path.exists() {
                let bytes = thumbnail::encode_card_thumbnail_at_width(&image, width).map_err(|error| error.to_string())?;
                fs::write(path, bytes).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    pub fn copy_missing_from(&self, source: &Self) -> Result<(), String> {
        for variant in VARIANTS {
            let (from, to) = (source.path(variant), self.path(variant));
            if from.exists() && !to.exists() {
                fs::copy(&from, to).map_err(|error| format!("{}: {error}", from.display()))?;
            }
        }
        Ok(())
    }

    pub fn publish(&self, destination: &Path) -> Result<(), String> {
        self.generate_cards()?;
        CoverAssets::new(destination, self.key).copy_missing_from(self)
    }

    pub fn remove(&self) -> Result<(), String> {
        for variant in VARIANTS {
            match fs::remove_file(self.path(variant)) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    }
}
