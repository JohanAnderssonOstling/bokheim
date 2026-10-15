//! Selection, search and annotation colours for the PDF renderer.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn packed_rgba(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderInteractionStyle {
    pub selection: RgbaColor,
    pub search_match: RgbaColor,
    pub active_search_match: RgbaColor,
    pub annotation: RgbaColor,
    pub active_annotation: RgbaColor,
    /// The passage being read aloud.
    pub reading: RgbaColor,
}

impl ReaderInteractionStyle {
    pub const LIBRARY_LIGHT: Self = Self {
        selection: RgbaColor::new(151, 188, 159, 112),
        search_match: RgbaColor::new(232, 193, 78, 116),
        active_search_match: RgbaColor::new(190, 116, 55, 164),
        annotation: RgbaColor::new(236, 204, 91, 104),
        active_annotation: RgbaColor::new(196, 126, 57, 148),
        reading: RgbaColor::new(151, 188, 159, 56),
    };
}

impl Default for ReaderInteractionStyle {
    fn default() -> Self {
        Self::LIBRARY_LIGHT
    }
}
