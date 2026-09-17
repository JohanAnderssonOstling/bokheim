//! Diagnose subject browsing on a disposable library database copy.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: probe_subject_root DATABASE_COPY")?;
    let conn = rusqlite::Connection::open(path)?;
    client_library::schema::init_db(&conn)?;
    let query = library_model::LibraryBrowseQuery {
        location: subject_projection::ROOT_SUBJECT_PATH.into(),
        search: String::new(),
        file_types: vec![library_model::LibraryFileTypeFilter::All],
        chip_sort: library_model::BrowseChipSort::Alphabetical,
        book_sort: library_model::BrowseBookSort::Alphabetical,
        languages: Vec::new(), hide_finished: false, include_direct_child_books: false,
    };
    let contents = client_library::browse::compacted_subject_contents(&conn, &query, false)?;
    println!("Subject root: {} categories, {} direct books", contents.children.len(), contents.books.len());
    for child in &contents.children { println!("{}: {} books", child.name, child.book_count); }
    if contents.children.is_empty() && contents.books.is_empty() { return Err("subject root is empty".into()); }
    let (grouped, groups) = client_library::browse::grouped_subject_contents(&conn, &query, false)?;
    println!("Grouped UI: {} categories, {} groups", grouped.children.len(), groups.len());
    if grouped.children.is_empty() && grouped.books.is_empty() { return Err("grouped subject root is empty".into()); }
    Ok(())
}
