//! Address normalization and typed-address history shared by native browsers.
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use url::Url;

const HISTORY_LIMIT: usize = 100;

/// Evaluated only after a main-frame page has finished loading.
pub const PAGE_METADATA_SCRIPT: &str = include_str!("page_metadata.js");

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PageMetadata {
    pub url: String,
    pub title: String,
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
}

impl PageMetadata {
    /// A readable label while the document's metadata is still loading.
    pub fn for_url(url: &str) -> Self {
        let parsed = web_url(url);
        let title = parsed.as_ref().and_then(Url::host_str)
            .map(|host| host.trim_start_matches("www.").to_owned()).unwrap_or_else(|| "Start page".into());
        let icon = parsed.and_then(|url| url.join("/favicon.ico").ok()).map(|url| url.to_string());
        Self { url: url.into(), title, icon, site_name: None }
    }

    pub fn display_name(&self) -> &str {
        self.site_name.as_deref().map(str::trim).filter(|name| !name.is_empty())
            .or_else(|| (!self.title.trim().is_empty()).then_some(self.title.trim())).unwrap_or("Web page")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VisitedPage {
    #[serde(flatten)]
    pub page: PageMetadata,
    pub visits: u64,
}

pub fn normalize(input: &str) -> Result<Url, String> {
    let input = input.trim();
    if input.is_empty() || ["javascript:", "data:", "file:", "about:", "mailto:", "intent:"].iter().any(|scheme| input.to_ascii_lowercase().starts_with(scheme)) {
        return Err("Enter a web address or search Google.".into());
    }
    let explicit = input.contains("://") || input.starts_with("//");
    let host = input.split(['/', '?', '#']).next().unwrap_or_default();
    let domain = host.contains('.') || host == "localhost" || host.starts_with("localhost:") || host.starts_with('[');
    if !explicit && (!domain || input.chars().any(char::is_whitespace)) {
        let mut search = Url::parse("https://www.google.com/search").map_err(|error| error.to_string())?;
        search.query_pairs_mut().append_pair("q", input);
        return Ok(search);
    }
    let address = if input.starts_with("//") {
        format!("https:{input}")
    } else if explicit {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&address).map_err(|_| "Enter a valid web address, such as example.com.")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("Enter an HTTP or HTTPS address without embedded login details.".into());
    }
    Ok(url)
}

pub struct AddressHistory {
    path: PathBuf,
    pub addresses: Vec<String>,
    visited: Vec<VisitedPage>,
}

impl AddressHistory {
    pub fn load(path: PathBuf) -> Self {
        let visited = read_visits(&path);
        Self { addresses: read(&path), path, visited }
    }

    /// Save typed addresses only; redirects and links do not fill suggestions.
    pub fn remember(&mut self, input: &str) -> Result<(), String> {
        let address = history_entry(input)?;
        // Merge other windows' latest entries before adding ours.
        self.addresses = read(&self.path);
        self.addresses.retain(|saved| saved != &address);
        self.addresses.insert(0, address);
        self.addresses.truncate(HISTORY_LIMIT);
        save(&self.path, &self.addresses)
    }

    pub fn suggestions(&self, input: &str) -> Vec<String> {
        let query = searchable(input.trim());
        self.addresses.iter().filter(|address| searchable(address).contains(&query)).take(6).cloned().collect()
    }

    /// Page visits are kept separately from typed-address suggestions, so
    /// following links contributes to shortcuts without filling the dropdown.
    pub fn record_visit(&mut self, mut page: PageMetadata) -> Result<(), String> {
        let mut url = web_url(&page.url).ok_or("Only web pages belong in browser history")?;
        url.set_fragment(None);
        page.url = url.to_string();
        page.title = page.title.trim().chars().take(200).collect();
        if page.title.is_empty() || page.title == page.url {
            page.title = "Untitled page".into();
        }
        page.icon = page.icon.and_then(|icon| web_url(&icon).map(|url| url.to_string()));
        self.visited = read_visits(&self.path);
        if let Some(saved) = self.visited.iter_mut().find(|saved| saved.page.url == page.url) {
            saved.page = page;
            saved.visits = saved.visits.saturating_add(1);
        } else {
            self.visited.push(VisitedPage { page, visits: 1 });
        }
        sort_visits(&mut self.visited);
        save(&self.path.with_extension("visits.json"), &self.visited)
    }

    /// Frequency first, then title alphabetically. No recency is recorded.
    pub fn most_visited(&self) -> &[VisitedPage] {
        &self.visited[..self.visited.len().min(8)]
    }

    pub fn page(&self, url: &str) -> Option<&PageMetadata> {
        self.visited.iter().find(|saved| saved.page.url == url).map(|saved| &saved.page)
    }
}

fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn web_url(input: &str) -> Option<Url> {
    let url = Url::parse(input).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
        && url.username().is_empty() && url.password().is_none()).then_some(url)
}

fn sort_visits(pages: &mut [VisitedPage]) {
    pages.sort_by(|a, b| b.visits.cmp(&a.visits)
        .then_with(|| a.page.title.to_lowercase().cmp(&b.page.title.to_lowercase()))
        .then_with(|| a.page.url.cmp(&b.page.url)));
}

fn read_visits(path: &Path) -> Vec<VisitedPage> {
    let mut pages: Vec<VisitedPage> = fs::read(path.with_extension("visits.json")).ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    pages.retain(|saved| web_url(&saved.page.url).is_some() && saved.visits > 0);
    for saved in &mut pages {
        saved.page.icon = saved.page.icon.take().filter(|icon| web_url(icon).is_some());
    }
    sort_visits(&mut pages);
    pages
}

fn searchable(value: &str) -> String {
    value.to_lowercase().trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").to_owned()
}

fn read(path: &Path) -> Vec<String> {
    let saved: Vec<String> = fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let mut addresses = Vec::new();
    for address in saved {
        if let Ok(address) = history_entry(&address) {
            if !addresses.contains(&address) {
                addresses.push(address);
            }
        }
        if addresses.len() == HISTORY_LIMIT {
            break;
        }
    }
    addresses
}

fn history_entry(input: &str) -> Result<String, String> {
    let url = normalize(input)?;
    if url.host_str() == Some("www.google.com") && url.path() == "/search"
        && url.query_pairs().any(|(key, value)| key == "q" && value == input.trim())
    {
        Ok(input.trim().to_owned())
    } else {
        Ok(url.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bare_domains_paths_and_ports_without_adding_www() {
        for (input, expected) in [
            (" example.com ", "https://example.com/"),
            ("example.com/books?q=rust#chapter", "https://example.com/books?q=rust#chapter"),
            ("www.example.com", "https://www.example.com/"),
            ("example.com:8443/books", "https://example.com:8443/books"),
            ("//example.com/books", "https://example.com/books"),
            ("http://localhost:8080/books", "http://localhost:8080/books"),
        ] {
            assert_eq!(normalize(input).unwrap().as_str(), expected);
        }
        for input in ["", "javascript:alert(1)", "file:///etc/passwd", "https://user:secret@example.com", "intent://book", "about:srcdoc"] {
            assert!(normalize(input).is_err(), "accepted {input}");
        }
    }

    #[test]
    fn searches_words_and_encodes_query_characters() {
        for input in ["rust", "rust books", "C++ & Rust", "books på svenska", "site:example.com rust"] {
            let url = normalize(input).unwrap();
            assert_eq!(url.host_str(), Some("www.google.com"));
            assert_eq!(url.path(), "/search");
            assert_eq!(url.query_pairs().collect::<Vec<_>>(), [("q".into(), input.into())]);
        }
    }

    #[test]
    fn history_survives_reopening_merges_windows_and_moves_repeats_to_front() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        let mut first = AddressHistory::load(path.clone());
        let mut second = AddressHistory::load(path.clone());
        first.remember("example.com").unwrap();
        second.remember("books.example.org").unwrap();
        second.remember("https://example.com/").unwrap();
        assert_eq!(AddressHistory::load(path).addresses, ["https://example.com/", "https://books.example.org/"]);
    }

    #[test]
    fn missing_browser_profile_directories_are_created_for_addresses_and_visits() {
        let directory = tempfile::tempdir().unwrap();
        let typed = directory.path().join("typed/profile/history.json");
        AddressHistory::load(typed.clone()).remember("example.com").unwrap();
        assert_eq!(AddressHistory::load(typed).addresses, ["https://example.com/"]);
        let visited = directory.path().join("visited/profile/history.json");
        AddressHistory::load(visited.clone()).record_visit(PageMetadata::for_url("https://example.com/")).unwrap();
        assert_eq!(AddressHistory::load(visited).most_visited()[0].visits, 1);
    }

    #[test]
    fn history_is_bounded_and_recovers_from_invalid_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        fs::write(&path, b"not json").unwrap();
        let mut history = AddressHistory::load(path.clone());
        assert!(history.addresses.is_empty());
        for index in 0..105 {
            history.remember(&format!("example.com/{index}")).unwrap();
        }
        assert_eq!(AddressHistory::load(path).addresses.len(), HISTORY_LIMIT);
        assert_eq!(history.addresses[0], "https://example.com/104");
    }

    #[test]
    fn remembers_search_text_without_saving_invalid_addresses() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        let mut history = AddressHistory::load(path.clone());
        history.remember("rust books").unwrap();
        assert!(history.remember("file:///etc/passwd").is_err());
        assert_eq!(AddressHistory::load(path).addresses, ["rust books"]);
    }

    #[test]
    fn suggestions_match_sites_and_searches_and_are_limited_to_six() {
        let directory = tempfile::tempdir().unwrap();
        let mut history = AddressHistory::load(directory.path().join("history.json"));
        for index in 0..8 { history.remember(&format!("www.example.com/book{index}")).unwrap(); }
        history.remember("Rust books").unwrap();
        assert_eq!(history.suggestions("").len(), 6);
        assert_eq!(history.suggestions(" WWW.EXAMPLE.COM/BOOK7 "), ["https://www.example.com/book7"]);
        assert_eq!(history.suggestions("RUST"), ["Rust books"]);
        assert!(history.suggestions("unknown.example").is_empty());
    }

    #[test]
    fn shortcuts_rank_by_visits_then_title_and_keep_typed_history_separate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        let mut first = AddressHistory::load(path.clone());
        let mut second = AddressHistory::load(path.clone());
        let page = |name: &str, title: &str| PageMetadata {
            url: format!("https://example.com/{name}"), title: title.into(), icon: Some("https://example.com/icon.png".into()), site_name: None,
        };
        first.remember("typed.example.com").unwrap();
        first.record_visit(page("z", "Zulu")).unwrap();
        second.record_visit(page("b", "beta")).unwrap();
        first.record_visit(page("a", "Alpha")).unwrap();
        second.record_visit(page("z", "Zulu renamed")).unwrap();
        let reopened = AddressHistory::load(path);
        let shortcuts = reopened.most_visited();
        assert_eq!(shortcuts.iter().map(|saved| (saved.page.title.as_str(), saved.visits)).collect::<Vec<_>>(),
            [("Zulu renamed", 2), ("Alpha", 1), ("beta", 1)]);
        assert_eq!(shortcuts[0].page.icon.as_deref(), Some("https://example.com/icon.png"));
        assert_eq!(reopened.addresses, ["https://typed.example.com/"]);
    }

    #[test]
    fn address_identity_prefers_site_name_and_handles_older_saved_metadata() {
        let mut page = PageMetadata::for_url("https://www.example.com/books?private=query#chapter");
        assert_eq!(page.display_name(), "example.com");
        assert_eq!(page.icon.as_deref(), Some("https://www.example.com/favicon.ico"));
        page.title = "A particular book".into();
        page.site_name = Some(" Example Books ".into());
        assert_eq!(page.display_name(), "Example Books");
        let old: PageMetadata = serde_json::from_str(r#"{"url":"https://example.com","title":"Old page","icon":null}"#).unwrap();
        assert_eq!(old.display_name(), "Old page");
        assert_eq!(PageMetadata::for_url("about:blank").display_name(), "Start page");
    }

    #[test]
    fn shortcuts_ignore_unsafe_pages_and_icons_and_collapse_fragments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        let mut history = AddressHistory::load(path.clone());
        for url in ["about:blank", "data:text/html,hello", "https://user:secret@example.com", "file:///tmp/page"] {
            assert!(history.record_visit(PageMetadata { url: url.into(), title: "Page".into(), icon: None, site_name: None }).is_err());
        }
        for fragment in ["one", "two"] {
            history.record_visit(PageMetadata { url: format!("https://example.com/#{fragment}"), title: "  ".into(), icon: Some("javascript:alert(1)".into()), site_name: None }).unwrap();
        }
        let reopened = AddressHistory::load(path);
        assert_eq!(reopened.most_visited().len(), 1);
        let saved = &reopened.most_visited()[0];
        assert_eq!(saved.page.url, "https://example.com/");
        assert_eq!(saved.page.title, "Untitled page");
        assert_eq!(saved.page.icon, None);
        assert_eq!(saved.visits, 2);
    }

    #[test]
    fn shortcuts_are_limited_to_eight_and_recover_from_invalid_storage() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        fs::write(path.with_extension("visits.json"), b"broken").unwrap();
        let mut history = AddressHistory::load(path);
        assert!(history.most_visited().is_empty());
        for index in (0..12).rev() {
            history.record_visit(PageMetadata { url: format!("https://example.com/{index}"), title: format!("Page {index:02}"), icon: None, site_name: None }).unwrap();
        }
        assert_eq!(history.most_visited().len(), 8);
        assert_eq!(history.most_visited()[0].page.title, "Page 00");
        assert_eq!(history.most_visited()[7].page.title, "Page 07");
    }
}
