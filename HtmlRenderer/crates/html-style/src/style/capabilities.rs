//! Renderer capability classification.
//!
//! Parser acceptance and renderer support are deliberately separate facts. A
//! declaration such as `opacity: 0.5` can be valid CSS while still being a
//! feature this renderer does not implement.

pub(crate) fn is_supported_property_name(name: &str) -> bool {
    matches!(
        name,
        "font-size"
            | "font-weight"
            | "font-style"
            | "font-family"
            | "font"
            | "line-height"
            | "letter-spacing"
            | "word-spacing"
            | "tab-size"
            | "text-align"
            | "text-align-last"
            | "text-indent"
            | "text-transform"
            | "font-variant-caps"
            | "text-decoration-line"
            | "text-decoration"
            | "vertical-align"
            | "white-space"
            | "list-style-type"
            | "list-style-position"
            | "list-style-image"
            | "list-style"
            | "color"
            | "background-color"
            | "background"
            | "width"
            | "height"
            | "min-width"
            | "min-height"
            | "max-width"
            | "max-height"
            | "margin-top"
            | "margin-bottom"
            | "margin-left"
            | "margin-right"
            | "margin"
            | "margin-block"
            | "margin-block-start"
            | "margin-block-end"
            | "margin-inline"
            | "margin-inline-start"
            | "margin-inline-end"
            | "padding-top"
            | "padding-bottom"
            | "padding-left"
            | "padding-right"
            | "padding"
            | "padding-block"
            | "padding-block-start"
            | "padding-block-end"
            | "padding-inline"
            | "padding-inline-start"
            | "padding-inline-end"
            | "border-width"
            | "border-top-width"
            | "border-right-width"
            | "border-bottom-width"
            | "border-left-width"
            | "border-color"
            | "border-top-color"
            | "border-right-color"
            | "border-bottom-color"
            | "border-left-color"
            | "border-style"
            | "border-top-style"
            | "border-right-style"
            | "border-bottom-style"
            | "border-left-style"
            | "border"
            | "border-top"
            | "border-bottom"
            | "border-left"
            | "border-right"
            | "border-spacing"
            | "float"
            | "clear"
            | "hyphens"
            | "word-break"
            | "overflow-wrap"
            | "word-wrap"
            | "box-sizing"
            | "page-break-before"
            | "page-break-after"
            | "page-break-inside"
            | "break-before"
            | "break-after"
            | "break-inside"
            | "display"
    )
}

pub fn property_name_is_supported(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    normalized.starts_with("--") || is_supported_property_name(&normalized)
}

#[cfg(test)]
mod tests {
    use super::property_name_is_supported;

    #[test]
    fn capability_queries_do_not_require_parser_ast_types() {
        assert!(property_name_is_supported("WIDTH"));
        assert!(property_name_is_supported("--book-accent"));
        assert!(property_name_is_supported("border-spacing"));
        assert!(!property_name_is_supported("opacity"));
    }
}
