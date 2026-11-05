use scraper::ElementRef;

use book_model::BookTocEntry;

/// Build a hierarchy from ordered entries whose nesting is described by levels.
pub(crate) fn nest_entries(entries: impl IntoIterator<Item = (usize, BookTocEntry)>) -> Vec<BookTocEntry> {
    let mut roots = Vec::new();
    let mut path: Vec<(usize, Vec<BookTocEntry>)> = Vec::new();
    for (level, entry) in entries {
        let level = level.min(31);
        while path.last().is_some_and(|(depth, _)| *depth >= level) {
            finish_level(&mut path, &mut roots);
        }
        path.push((level, vec![entry]));
    }
    while !path.is_empty() {
        finish_level(&mut path, &mut roots);
    }
    roots
}

fn finish_level(path: &mut Vec<(usize, Vec<BookTocEntry>)>, roots: &mut Vec<BookTocEntry>) {
    let Some((_, mut children)) = path.pop() else { return };
    if let Some((_, parent)) = path.last_mut() {
        if let Some(last) = parent.last_mut() {
            last.children.append(&mut children);
        }
    } else {
        roots.append(&mut children);
    }
}

pub(crate) fn html_entries(root: ElementRef<'_>, resolve: impl Fn(ElementRef<'_>) -> Option<String>) -> Vec<BookTocEntry> {
    nest_entries(
        root.descendants()
            .filter_map(ElementRef::wrap)
            .filter(|element| element.id() != root.id() && element.value().name() == "a" && (element.value().attr("href").is_some() || element.value().attr("filepos").is_some()))
            .filter_map(|element| {
                let link = resolve(element)?;
                let title = element.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
                if title.is_empty() || title.len() > 4096 {
                    return None;
                }
                let level = element.ancestors().take_while(|node| node.id() != root.id()).filter_map(ElementRef::wrap).filter(|element| matches!(element.value().name(), "ol" | "ul")).count();
                Some((level, BookTocEntry { title, target: link, children: Vec::new() }))
            })
            .take(8192),
    )
}

/// Container targets must resolve independently of the visible document.
pub(crate) fn root_href(href: String) -> String {
    if href.is_empty() || href.starts_with('/') || href.split('#').next().is_some_and(|path| path.contains(':')) { href } else { format!("/{href}") }
}

/// Navigation lists have little value when reading the book aloud. Match
/// complete titles so ordinary chapters mentioning contents remain readable.
pub fn is_navigation_section_title(title: &str) -> bool {
    let normalized: String = title
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "contents"
            | "tableofcontents"
            | "toc"
            | "detailedcontents"
            | "briefcontents"
            | "contentsataglance"
            | "listofillustrations"
            | "listoffigures"
            | "listoftables"
            | "listofmaps"
            | "innehåll"
            | "innehållsförteckning"
    )
}

