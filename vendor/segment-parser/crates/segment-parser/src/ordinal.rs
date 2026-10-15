use structure::OrdinalStyle;

pub fn format_ordinal(style: OrdinalStyle, value: u32) -> String {
    match style {
        OrdinalStyle::EnglishSuffix => format!("{value}{}", english_ordinal_suffix(value)),
        OrdinalStyle::SwedishColonSuffix => format!("{value}:{}", swedish_ordinal_suffix(value)),
        OrdinalStyle::GermanPeriod => format!("{value}."),
    }
}

pub(crate) fn english_ordinal_suffix(value: u32) -> &'static str {
    if matches!(value % 100, 11..=13) {
        return "th";
    }
    match value % 10 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

pub(crate) fn swedish_ordinal_suffix(value: u32) -> &'static str {
    if matches!(value % 100, 11 | 12) {
        return "e";
    }
    if matches!(value % 10, 1 | 2) { "a" } else { "e" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_supported_ordinal_styles() {
        assert_eq!(format_ordinal(OrdinalStyle::EnglishSuffix, 21), "21st");
        assert_eq!(format_ordinal(OrdinalStyle::EnglishSuffix, 11), "11th");
        assert_eq!(format_ordinal(OrdinalStyle::SwedishColonSuffix, 2), "2:a");
        assert_eq!(format_ordinal(OrdinalStyle::SwedishColonSuffix, 12), "12:e");
        assert_eq!(format_ordinal(OrdinalStyle::GermanPeriod, 3), "3.");
    }
}
