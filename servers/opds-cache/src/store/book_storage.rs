use super::{params, CatalogBookFormat, CatalogBook, CatalogBookAction, CatalogSourceId, CatalogStoreError, Transaction};

pub struct BookInventory {
    pub book_format: Option<&'static str>,
    pub importable: bool,
    pub has_upstream_cover: bool,
}

pub struct BookExtraction<'a> {
    pub acquisition_url: &'a str,
    pub book_format: CatalogBookFormat,
}

pub fn book_key(book: &CatalogBook) -> (&str, &str) {
    (&book.identity.edition_id, book.identity.work_id.as_deref().unwrap_or(""))
}

pub fn book_extraction(book: &CatalogBook) -> Option<BookExtraction<'_>> {
    book.actions.iter().find_map(|action| match action {
        CatalogBookAction::Import { request } if matches!(request.book_format, CatalogBookFormat::Epub | CatalogBookFormat::Pdf | CatalogBookFormat::Mobi) => {
            Some(BookExtraction { acquisition_url: &request.download_url, book_format: request.book_format })
        }
        _ => None,
    })
}

pub fn book_inventory(book: &CatalogBook) -> BookInventory {
    let book_format = book.actions.iter().find_map(|action| match action {
        CatalogBookAction::Import { request } => Some(book_format_storage(request.book_format)),
    });
    BookInventory { book_format, importable: book_format.is_some(), has_upstream_cover: book.image_url.as_deref().is_some_and(|url| url.starts_with("https://") || url.starts_with("http://")) }
}

pub fn book_entry_id(transaction: &Transaction<'_>, source_id: &CatalogSourceId, book: &CatalogBook, payload_hash: &str) -> Result<i64, CatalogStoreError> {
    let (edition_id, work_id) = book_key(book);
    transaction.execute("INSERT OR IGNORE INTO catalog_book_entry(source_id,edition_id,work_id,payload_hash) VALUES(?1,?2,?3,?4)", params![source_id.as_str(), edition_id, work_id, payload_hash])?;
    transaction
        .query_row("SELECT entry_id FROM catalog_book_entry WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND payload_hash=?4", params![source_id.as_str(), edition_id, work_id, payload_hash], |row| row.get(0))
        .map_err(Into::into)
}

pub fn book_format_storage(format: CatalogBookFormat) -> &'static str {
    match format {
        CatalogBookFormat::Epub => "epub",
        CatalogBookFormat::Pdf => "pdf",
        CatalogBookFormat::Mobi => "mobi",
        CatalogBookFormat::M4b => "m4b",
    }
}

pub fn catalog_book_format_from_storage(value: &str) -> Option<CatalogBookFormat> {
    match value {
        "epub" => Some(CatalogBookFormat::Epub),
        "pdf" => Some(CatalogBookFormat::Pdf),
        "mobi" => Some(CatalogBookFormat::Mobi),
        "m4b" => Some(CatalogBookFormat::M4b),
        _ => None,
    }
}
