use std::io;
use std::sync::Arc;

use html_view_core::{ResourceMetadata, ResourceProvider};

struct DescriptionProvider {
    document: Vec<u8>,
}

impl ResourceProvider for DescriptionProvider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        (uri == "description.html").then(|| self.document.clone()).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, uri.to_owned()))
    }

    fn metadata(&self, uri: &str) -> io::Result<ResourceMetadata> {
        if uri == "description.html" { Ok(ResourceMetadata { media_type: Some("text/html".to_owned()), charset: Some("utf-8".to_owned()) }) } else { Err(io::Error::new(io::ErrorKind::NotFound, uri.to_owned())) }
    }

    fn exists(&self, uri: &str) -> bool {
        uri == "description.html"
    }

    fn resolve(&self, _base: &str, href: &str) -> String {
        href.to_owned()
    }

    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        Ok(vec!["description.html".to_owned()])
    }
}

pub(crate) fn description_document(description: &str) -> Vec<u8> {
    let lower = description.to_ascii_lowercase();
    let authored_html = ["<p", "<br", "<div", "<em", "<strong", "<ul", "<ol", "<blockquote", "<span", "<a "].iter().any(|tag| lower.contains(tag));
    let content = if authored_html { description.to_owned() } else { description.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\n', "<br/>") };
    format!("<!doctype html><html><head><meta charset=\"utf-8\"><style>html,body,article{{margin:0;padding:0;background:transparent}} article{{text-align:justify;hyphens:auto}}</style></head><body><article>{content}</article></body></html>").into_bytes()
}

pub(crate) fn description_provider(description: &str) -> Arc<dyn ResourceProvider> {
    Arc::new(DescriptionProvider { document: description_document(description) })
}

#[cfg(test)]
mod tests {
    use super::description_document;

    #[test]
    fn plain_description_is_safe_html_with_preserved_line_breaks() {
        let html = String::from_utf8(description_document("Books & history\nSecond line")).unwrap();
        assert!(html.contains("Books &amp; history<br/>Second line"));
    }

    #[test]
    fn authored_description_markup_is_preserved() {
        let html = String::from_utf8(description_document("<p>A <em>short</em> description.</p>")).unwrap();
        assert!(html.contains("<p>A <em>short</em> description.</p>"));
    }
}
