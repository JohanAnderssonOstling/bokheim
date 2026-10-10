//! The parsers against saved feeds: Project Gutenberg (OPDS 1.2), Readium's
//! test catalog (OPDS 2) and a hand-made acquisition feed.
use library_opds::{opensearch_template, parse, Search, EPUB};
use url::Url;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn url(value: &str) -> Url {
    Url::parse(value).unwrap()
}

#[test]
fn gutenberg_root_is_navigation_with_opensearch() {
    let feed = parse(&fixture("gutenberg-root.xml"), Some("application/atom+xml;profile=opds-catalog"), &url("https://www.gutenberg.org/ebooks.opds/")).unwrap();
    assert_eq!(feed.title, "Project Gutenberg");
    assert_eq!(feed.navigation.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), ["Popular", "Latest", "Random"]);
    assert_eq!(feed.navigation[0].href, url("https://www.gutenberg.org/ebooks/search.opds/?sort_order=downloads"));
    assert!(feed.publications.is_empty());
    assert_eq!(feed.search, Some(Search::Description(url("https://www.gutenberg.org/catalog/osd-books.xml"))));
}

#[test]
fn gutenberg_search_results_link_each_book_to_its_page() {
    let feed = parse(&fixture("gutenberg-search.xml"), None, &url("https://www.gutenberg.org/ebooks/search.opds/?sort_order=downloads")).unwrap();
    // Each book is a link to its own page, which holds the downloads.
    assert_eq!(feed.navigation.len(), 25);
    assert!(feed.navigation.iter().any(|link| link.title == "Pride and Prejudice" && link.href == url("https://www.gutenberg.org/ebooks/1342.opds")));
    assert_eq!(feed.next, Some(url("https://www.gutenberg.org/ebooks/search.opds/?sort_order=downloads&start_index=26")));
}

#[test]
fn gutenberg_book_page_offers_open_epubs() {
    let feed = parse(&fixture("gutenberg-book.xml"), Some("application/atom+xml"), &url("https://www.gutenberg.org/ebooks/1342.opds")).unwrap();
    assert_eq!(feed.publications.iter().map(|publication| publication.id.as_str()).collect::<Vec<_>>(), ["urn:gutenberg:1342:2", "urn:gutenberg:1342:3"]);
    let first = &feed.publications[0];
    assert_eq!(first.title, "Pride and Prejudice");
    assert_eq!(first.authors, ["Austen, Jane"]);
    let download = first.download.as_ref().unwrap();
    assert_eq!(download.media_type, EPUB);
    assert_eq!(download.href, url("https://www.gutenberg.org/ebooks/1342.epub.noimages"));
    // The full-size image over the thumbnail.
    assert_eq!(first.cover, Some(url("https://www.gutenberg.org/cache/epub/1342/pg1342.cover.medium.jpg")));
}

#[test]
fn opensearch_description_gives_the_feed_template() {
    let template = opensearch_template(std::str::from_utf8(&fixture("gutenberg-opensearch.xml")).unwrap()).unwrap();
    assert_eq!(template, "http://m.gutenberg.org/ebooks/search.opds/?query={searchTerms}");
    assert_eq!(Search::url(&template, "pride & prejudice").unwrap(), url("http://m.gutenberg.org/ebooks/search.opds/?query=pride+%26+prejudice"));
}

#[test]
fn acquisition_feed_offers_open_downloads_facets_paging_and_search() {
    let feed = parse(&fixture("acquisition.xml"), Some("application/atom+xml"), &url("http://calibre.local:8080/opds")).unwrap();
    assert_eq!(feed.publications.len(), 3);
    // XHTML content is kept as markup.
    assert!(feed.publications[0].description.starts_with("<div") && feed.publications[0].description.contains("<p>In a hole in the ground there lived a hobbit.</p>"));
    // EPUB is chosen over the PDF the entry also offers.
    assert_eq!(feed.publications[0].download.as_ref().unwrap().href, url("http://calibre.local:8080/get/epub/12/Calibre"));
    // A DRM-only and a buy-only entry are listed but offer nothing to download.
    assert!(feed.publications[1].download.is_none());
    assert!(feed.publications[2].download.is_none());
    assert_eq!(feed.navigation.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), ["Authors"]);
    assert_eq!(feed.next, Some(url("http://calibre.local:8080/opds/navcatalog/4e6577657374?library_id=Calibre&offset=2")));
    assert_eq!(feed.facets.len(), 1);
    assert_eq!(feed.facets[0].title, "Sort by");
    assert_eq!(feed.facets[0].facets.iter().map(|facet| (facet.title.as_str(), facet.active)).collect::<Vec<_>>(), [("Newest", true), ("Title", false)]);
    let Some(Search::Template(template)) = &feed.search else { panic!("expected a search template, got {:?}", feed.search) };
    assert_eq!(Search::url(template, "hobbit").unwrap(), url("http://calibre.local:8080/opds/search/hobbit?library_id=Calibre"));
}

#[test]
fn opds2_home_flattens_groups_into_the_page() {
    let feed = parse(&fixture("opds2-home.json"), Some("application/opds+json"), &url("https://test.opds.io/2.0/home.json")).unwrap();
    assert_eq!(feed.title, "OPDS 2.0 Test Catalog");
    assert_eq!(feed.navigation[0].title, "Explore (Navigation Feed)");
    // 8 links of its own, "French Classics" (a group with its own page) and
    // the 2 links of "More Navigation".
    assert_eq!(feed.navigation.len(), 11);
    assert!(feed.navigation.iter().any(|link| link.title == "French Classics" && link.href == url("https://test.opds.io/2.0/publications.json")));
    // 2 of its own and the 5 and 3 of its groups.
    assert_eq!(feed.publications.len(), 10);
}

#[test]
fn opds2_publication_reads_contributors_and_open_download() {
    let feed = parse(&fixture("opds2-publications.json"), None, &url("https://test.opds.io/2.0/publications.json")).unwrap();
    assert_eq!(feed.publications.len(), 14);
    let verne = &feed.publications[0];
    assert_eq!(verne.title, "Voyage au centre de la Terre");
    assert_eq!(verne.subtitle.as_deref(), Some("Un super sous-titre"));
    assert_eq!(verne.authors, ["Jules Verne"]);
    assert!(verne.description.starts_with("Le professeur Lidenbrock"));
    assert_eq!(verne.download.as_ref().unwrap().href, url("https://test.opds.io/assets/centredelaterre/file.epub"));
}
