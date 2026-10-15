//! One OPDS (Atom) document's entries: what `add` needs from each.
use reqwest::Url;

const ATOM: &str = "http://www.w3.org/2005/Atom";

pub struct Entry {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    /// The first EPUB acquisition link.
    pub epub: Option<Url>,
    pub image: Option<Url>,
    /// Subject headings by name, and Library of Congress classes by code.
    pub lcsh: Vec<String>,
    pub lcc: Vec<String>,
}

pub fn entries(xml: &str, base: &Url) -> Result<Vec<Entry>, String> {
    let document = roxmltree::Document::parse(xml).map_err(|e| format!("not an OPDS document: {e}"))?;
    let atom = |node: &roxmltree::Node, name: &str| node.tag_name().namespace() == Some(ATOM) && node.tag_name().name() == name;
    let text = |node: &roxmltree::Node, name: &str| node.children().find(|child| atom(child, name)).and_then(|child| child.text()).map(|text| text.trim().to_owned());
    let mut entries = Vec::new();
    for entry in document.root_element().children().filter(|node| atom(node, "entry")) {
        let link = |rel: &str, kind: Option<&str>| {
            entry.children().filter(|node| atom(node, "link") && node.attribute("rel") == Some(rel) && kind.is_none_or(|kind| node.attribute("type") == Some(kind))).find_map(|node| base.join(node.attribute("href")?).ok())
        };
        let categories = |scheme: &str| entry.children().filter(|node| atom(node, "category") && node.attribute("scheme").is_some_and(|value| value.ends_with(scheme))).filter_map(|node| node.attribute("term")).map(str::to_owned).collect();
        entries.push(Entry {
            id: text(&entry, "id").ok_or("an OPDS entry has no id")?,
            title: text(&entry, "title").unwrap_or_default(),
            authors: entry.children().filter(|node| atom(node, "author")).filter_map(|author| text(&author, "name")).collect(),
            epub: link("http://opds-spec.org/acquisition", Some("application/epub+zip")),
            image: link("http://opds-spec.org/image", None),
            lcsh: categories("/LCSH"),
            lcc: categories("/LCC"),
        });
    }
    Ok(entries)
}
