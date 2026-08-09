mod paint;
mod text;
mod view;

pub use text::TextShapeCacheStats;
pub use view::{HtmlView, PaintCacheStats};

#[cfg(test)]
mod boundary_tests {
    #[test]
    fn gpui_adapter_has_no_direct_html_layout_or_style_dependencies() {
        let manifest = include_str!("../Cargo.toml");
        let dependencies = manifest.lines().filter_map(|line| line.split_once('=').map(|(name, _)| name.trim()));
        let forbidden = ["html-layout", "html-style", "html-style-model", "html-pipeline", "html-dom"];
        let violations = dependencies.filter(|name| forbidden.contains(name)).collect::<Vec<_>>();
        assert!(violations.is_empty(), "GPUI must consume the neutral text_backend contract, not HTML implementation crates: {violations:?}");
    }

    #[test]
    fn gpui_text_backend_contains_no_html_semantic_vocabulary() {
        let source = include_str!("text.rs");
        for forbidden in ["use html_view_core::{", "html_layout", "html_style", "html_pipeline", "html_dom", "StyleStringId", "letter_spacing", "word_spacing", "justification"] {
            assert!(!source.contains(forbidden), "GPUI text backend must not reference HTML-layer concept {forbidden:?}");
        }
    }
}
