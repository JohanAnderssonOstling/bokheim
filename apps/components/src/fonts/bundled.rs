//! Metadata shared by packaging and on-demand font registration.
pub struct BundledFamily {
    pub name: &'static str,
    pub files: &'static [&'static str],
}

pub const FAMILIES: &[BundledFamily] = &[
    BundledFamily { name: "NV Garamond", files: &["NV_Garamond-Regular.ttf", "NV_Garamond-Bold.ttf", "NV_Garamond-Italic.ttf", "NV_Garamond-BoldItalic.ttf"] },
    BundledFamily { name: "Raleway", files: &["Raleway-Regular.ttf", "Raleway-Bold.ttf", "Raleway-Italic.ttf", "Raleway-BoldItalic.ttf"] },
    BundledFamily { name: "Bodoni Moda", files: &["BodoniModa-Regular.ttf", "BodoniModa-SemiBold.ttf"] },
    BundledFamily { name: "Droid Sans Fallback", files: &["DroidSansFallback.ttf"] },
    #[cfg(target_os = "android")]
    BundledFamily { name: "Noto Sans", files: &["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"] },
];
