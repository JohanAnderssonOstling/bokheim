use app_contract::library::BookCardRow;

/// One directly browsable child returned by a database hierarchy query.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct BrowseRow {
    pub id: String,
    pub name: String,
    #[specta(type = specta_typescript::Number)]
    pub book_count: usize,
    #[specta(type = specta_typescript::Number)]
    pub downloaded_book_count: usize,
}

/// Direct database contents for one hierarchy location.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, specta::Type)]
pub struct BrowseContents {
    pub children: Vec<BrowseRow>,
    pub books: Vec<BookCardRow>,
}
