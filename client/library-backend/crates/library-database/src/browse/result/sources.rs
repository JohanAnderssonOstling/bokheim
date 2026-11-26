//! Source adapters prepare identities and navigation without hydrating cards.
use super::*;
use crate::browse::{FoldersSql, parse_uuid_column};
use crate::query_methods::text_column;
use library_model::{BrowseBookSort, HomeShelf};
use std::collections::HashSet;

impl Database {
    pub(super) fn prepare_browse_selection(&self, location: BrowseLocation, options: &BrowseOptions, catalog: Option<&Connection>) -> Result<Selection, DatabaseError> {
        match location {
            BrowseLocation::Folder(directory) => return self.prepare_folder_selection(directory, options),
            BrowseLocation::Home(shelf) => return self.prepare_home_selection(shelf, options),
            _ => {}
        }
        let mut metadata = LibraryBrowseData::default();
        let mut sections = Vec::new();
        let searched = !options.search.is_empty();
        match location {
            BrowseLocation::Author(author) | BrowseLocation::CatalogAuthor(author) => {
                let author_query = self.author_query(catalog, options)?;
                if author.is_none() {
                    let _stage = Stage::new("author heading aggregates");
                    metadata.contents.children = author_query.headings()?;
                } else if let Some(id) = author {
                    if let Some(name) = author_query.name(id)? {
                        let location = if catalog.is_some() { BrowseLocation::CatalogAuthor(Some(id)) } else { BrowseLocation::Author(Some(id)) };
                        metadata.path.push(BrowsePathSegment { id: location, name });
                    }
                }
                let direct_order = if author.is_some() || searched {
                    let _stage = Stage::new("author direct ordering");
                    author_query.ordered(author)?
                } else {
                    Vec::new()
                };
                (metadata.language_counts, metadata.format_counts) = match catalog {
                    Some(catalog) => self.catalog_facets(catalog, options)?,
                    None => self.library_facets(options)?,
                };
                return Ok(Selection { sections, metadata, books: SelectedBooks::Ordered { direct: direct_order.into_iter().map(Item::from).collect(), section_orders: Orders::new() } });
            }
            BrowseLocation::Subject(route) | BrowseLocation::Catalog(route) => self.prepare_subject_selection(route, options, catalog),
            BrowseLocation::Notebook(sort) => {
                let direct = self.notebook_book_ids(options, sort)?.into_iter().map(Item::from).collect();
                return Ok(Selection { sections, metadata, books: SelectedBooks::Ordered { direct, section_orders: Orders::new() } });
            }
            BrowseLocation::TextSearch(sort) => {
                let direct = self.text_search_ids(options, sort)?.into_iter().map(|(book, offsets)| Item { book, directory: None, text_offsets: Some(offsets) }).collect();
                return Ok(Selection { sections, metadata, books: SelectedBooks::Ordered { direct, section_orders: Orders::new() } });
            }
            BrowseLocation::Books => {
                let _stage = Stage::new("library book selection and ordering");
                let direct_order = self.author_query(None, options)?.all_ordered()?.into_iter().map(Item::from).collect();
                (metadata.language_counts, metadata.format_counts) = self.library_facets(options)?;
                return Ok(Selection { sections, metadata, books: SelectedBooks::Ordered { direct: direct_order, section_orders: Orders::new() } });
            }
            _ => Err(DatabaseError::message("unsupported browse result scope")),
        }
    }

    fn prepare_subject_selection(&self, route: i64, options: &BrowseOptions, catalog: Option<&Connection>) -> Result<Selection, DatabaseError> {
        let source = if catalog.is_some() { Books::Catalog } else { Books::Library };
        let snapshot = self.subject_snapshot(source, catalog, Some(options))?;
        let mut metadata = LibraryBrowseData::default();
        let mut sections = Vec::new();
        let mut eligible = BTreeSet::new();
        let mut direct = BTreeSet::new();
        if let Some(index) = snapshot.index(route) {
            metadata.contents.children = snapshot.children(index, options, catalog.is_some());
            eligible = snapshot.members(index).intersection(&snapshot.matching.books).copied().collect();
            direct = snapshot.selected(index, !options.search.is_empty());
            if options.search.is_empty() {
                for parent in &metadata.contents.children {
                    let (BrowseLocation::Subject(route) | BrowseLocation::Catalog(route)) = parent.id else { continue };
                    let index = snapshot.index(route).expect("listed subject belongs to the snapshot");
                    let children = snapshot.children(index, options, catalog.is_some());
                    let books = snapshot.selected(index, false);
                    if !children.is_empty() || !books.is_empty() {
                        sections.push(SectionSelection { parent: parent.clone(), children, books });
                    }
                }
            }
            metadata.path = snapshot.hierarchy.path(index);
            if let Some((_, above)) = metadata.path.split_last() {
                let unsearched = BrowseOptions { search: String::new(), ..options.clone() };
                for location in std::iter::once(source.location(0)).chain(above.iter().map(|row| row.id)) {
                    let (BrowseLocation::Subject(route) | BrowseLocation::Catalog(route)) = location else { continue };
                    let index = snapshot.index(route).expect("ancestor belongs to the snapshot");
                    metadata.ancestors.push((location, snapshot.children(index, &unsearched, catalog.is_some())));
                }
            }
        }
        (metadata.language_counts, metadata.format_counts) = match catalog {
            Some(catalog) => self.catalog_facets(catalog, options)?,
            None => self.library_facets(options)?,
        };
        Ok(Selection { sections, metadata, books: SelectedBooks::Membership { eligible, direct } })
    }

    fn prepare_home_selection(&self, shelf: Option<HomeShelf>, options: &BrowseOptions) -> Result<Selection, DatabaseError> {
        let prepared = self.home_shelves(options)?;
        let mut metadata = LibraryBrowseData::default();
        let mut sections = Vec::new();
        let mut section_orders = Orders::new();
        let direct = match shelf {
            Some(shelf) => prepared.shelves.into_iter().find(|found| found.shelf == shelf).map(|found| found.books).unwrap_or_default(),
            None if options.search.is_empty() => {
                for found in prepared.shelves.into_iter().filter(|found| !found.books.is_empty()) {
                    let parent = BrowseRow {
                        id: BrowseLocation::Home(Some(found.shelf)),
                        name: found.shelf.label().to_owned(),
                        path: None,
                        book_count: found.books.len(),
                        downloaded_book_count: found.books.iter().filter(|id| prepared.matching.downloaded.contains(id)).count(),
                        requested_book_count: None,
                        size_bytes: None,
                    };
                    metadata.contents.children.push(parent.clone());
                    section_orders.insert(parent.id, Arc::new(found.books.iter().copied().map(Item::from).collect()));
                    sections.push(SectionSelection { parent, children: Vec::new(), books: found.books.into_iter().collect() });
                }
                Vec::new()
            }
            None => {
                let mut seen = HashSet::new();
                let direct = prepared.shelves.into_iter().flat_map(|found| found.books).filter(|id| seen.insert(*id)).collect::<Vec<_>>();
                // Individual shelves already have their book order. Only a
                // merged search needs ordering across shelf boundaries.
                if options.book_sort != BrowseBookSort::DefaultOrder { self.ordered_books(&direct.iter().copied().collect(), options)? } else { direct }
            }
        };
        metadata.path = shelf.map(|shelf| BrowsePathSegment { id: BrowseLocation::Home(Some(shelf)), name: shelf.label().to_owned() }).into_iter().collect();
        (metadata.language_counts, metadata.format_counts) = self.library_facets(options)?;
        Ok(Selection { sections, metadata, books: SelectedBooks::Ordered { direct: direct.into_iter().map(Item::from).collect(), section_orders } })
    }

    fn prepare_folder_selection(&self, directory: DirId, options: &BrowseOptions) -> Result<Selection, DatabaseError> {
        let tree = self.folder_tree(directory, options)?;
        let scope = tree.scope(options);
        let mut metadata = LibraryBrowseData::default();
        metadata.contents.children = self.folder_children(scope, &tree.counts, options)?;
        let mut directories = scope.iter().copied().collect::<BTreeSet<_>>();
        if options.search.is_empty() {
            directories.extend(metadata.contents.children.iter().filter_map(|row| row.id.folder()));
        }
        let mut placements = HashMap::<DirId, Vec<Item>>::new();
        self.connection.folder_placements(&serde_json::to_string(&directories).map_err(DatabaseError::operation)?, |row| {
            let directory = parse_uuid_column(text_column(row, 1)?, 1)?;
            placements.entry(directory).or_default().push(Item { book: row.get(0)?, directory: Some(directory), text_offsets: None });
            Ok(())
        })?;
        let with_books = placements.keys().copied().collect::<HashSet<_>>();
        let candidates = placements.values().flatten().map(|item| item.book).collect();
        let matching = self.matching_books(&candidates, options)?;
        let selected = if options.search.is_empty() { &matching.books } else { &matching.text_matches };
        let ordered = self.ordered_books(selected, options)?;
        let positions = ordered.iter().enumerate().map(|(position, &id)| (id, position)).collect::<HashMap<_, _>>();
        for items in placements.values_mut() {
            items.retain(|item| selected.contains(&item.book));
            items.sort_unstable_by_key(|item| (positions[&item.book], item.directory));
        }
        let scope = scope.iter().copied().collect::<HashSet<_>>();
        let mut direct = placements.iter().filter(|(directory, _)| scope.contains(directory)).flat_map(|(_, items)| items.iter().cloned()).collect::<Vec<_>>();
        direct.sort_unstable_by_key(|item| (positions[&item.book], item.directory));
        let mut sections = Vec::new();
        let mut wrappers = Vec::new();
        let mut section_orders = Orders::new();
        if options.search.is_empty() {
            for parent in &metadata.contents.children {
                let Some(directory) = parent.id.folder() else { continue };
                let children = self.folder_children(&[directory], &tree.counts, options)?;
                let items = placements.remove(&directory).unwrap_or_default();
                let books = items.iter().map(|item| item.book).collect();
                section_orders.insert(parent.id, Arc::new(items));
                let wrapper = !with_books.contains(&directory) && !children.is_empty();
                let section = SectionSelection { parent: parent.clone(), children, books };
                if wrapper {
                    wrappers.push(section);
                } else {
                    sections.push(section);
                }
            }
            sections.extend(wrappers);
        }
        metadata.path = self.browse_directory_path(&directory)?;
        metadata.ancestors = self.folder_ancestors(&metadata.path, options)?;
        (metadata.language_counts, metadata.format_counts) = self.library_facets(options)?;
        Ok(Selection { sections, metadata, books: SelectedBooks::Ordered { direct, section_orders } })
    }
}
