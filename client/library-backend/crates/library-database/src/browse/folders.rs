//! Folder browsing: hierarchy, contents, facets, paths.

use crate::browse::{invalid_integer_column, parse_uuid_column, BrowseRow, FoldersSql};
use crate::{Database, DatabaseError};
use library_model::BrowseLocation;
use std::collections::{BTreeMap, HashMap, HashSet};
use sync_common::{DirId, ROOT_DIR_ID};

impl Database {
    pub(super) fn browse_folder(&self, directory_id: DirId, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<super::LibraryBrowseData, DatabaseError> {
        let options = super::BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let tree = self.folder_tree(&directory_id.to_string(), &options)?;
            let contents = self.folder_contents(tree.scope(&options), &tree.counts, &options)?;
            let sections = if options.search.is_empty() {
                {
                    let parents: &[BrowseRow] = &contents.children;
                    let counts: &FolderCounts = &tree.counts;
                    let options: &super::BrowseOptions = &options;
                    let ids = json(&parents.iter().filter_map(|row| if let BrowseLocation::Folder(id) = row.id { Some(id) } else { None }).map(|id| id.to_string()).collect::<Vec<_>>())?;
                    let mut with_books = HashSet::new();
                    self.connection.folder_placements(&ids, |row| {
                        with_books.insert(row.get::<_, String>(1)?);
                        Ok(())
                    })?;
                    let (mut plain, mut wrappers) = (Vec::new(), Vec::new());
                    for parent in parents {
                        let BrowseLocation::Folder(id) = parent.id else { continue };
                        let key = id.to_string();
                        let contents = self.folder_contents(std::slice::from_ref(&key), counts, options)?;
                        let wrapper = !with_books.contains(&key) && !contents.children.is_empty();
                        let section = library_model::BrowseSection { parent: parent.clone(), children: contents.children, books: contents.books };
                        if wrapper {
                            wrappers.push(section);
                        } else {
                            plain.push(section);
                        }
                    }
                    plain.extend(wrappers);
                    plain
                }
            } else {
                Vec::new()
            };
            let (language_counts, format_counts) = self.library_facets(&options)?;
            let path = self.browse_directory_path(&directory_id)?;
            let ancestors = self.folder_ancestors(&path, &options)?;
            Ok(super::LibraryBrowseData { contents, sections, path, ancestors, language_counts, format_counts, feed: None })
        })
    }

    fn folder_contents(&self, scope: &[String], counts: &FolderCounts, options: &super::BrowseOptions) -> Result<library_model::BrowseContents, DatabaseError> {
        let children = self.folder_children(scope, counts, options)?;
        let books = {
            let mut placements = BTreeMap::<i64, Vec<uuid::Uuid>>::new();
            self.connection.folder_placements(&json(scope)?, |row| {
                placements.entry(row.get(0)?).or_default().push(parse_uuid_column(row.get(1)?, 1)?);
                Ok(())
            })?;
            let matching = self.matching_books(&placements.keys().copied().collect(), options)?;
            let selected = if options.search.is_empty() { matching.books } else { matching.text_matches };
            let (mut ids, mut sources) = (Vec::new(), Vec::new());
            for book in self.ordered_books(&selected, options)? {
                let mut directories = placements.remove(&book).unwrap_or_default();
                directories.sort();
                for directory in directories {
                    ids.push(book);
                    sources.push(directory);
                }
            }
            let mut cards = self.load_cards(&ids)?;
            for (card, source) in cards.iter_mut().zip(sources) {
                card.source_directory = Some(source);
            }
            cards
        };
        Ok(library_model::BrowseContents { children, books })
    }

    pub(super) fn browse_folder_contents(&self, folder_id: DirId, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        let options = super::BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let tree = self.folder_tree(&folder_id.to_string(), &options)?;
            self.folder_contents(tree.scope(&options), &tree.counts, &options)
        })
    }

    /// The children of the root and of each folder on `path` above the last.
    /// Their counts need the whole library's tree, not only the subtree below
    /// the page's folder, so it is counted once from the root.
    fn folder_ancestors(&self, path: &[library_model::BrowsePathSegment], options: &super::BrowseOptions) -> Result<Vec<(BrowseLocation, Vec<BrowseRow>)>, DatabaseError> {
        let Some((_, above)) = path.split_last() else { return Ok(Vec::new()) };
        let options = super::BrowseOptions { search: String::new(), ..options.clone() };
        let tree = self.folder_tree(&ROOT_DIR_ID.to_string(), &options)?;
        let mut ancestors = Vec::with_capacity(above.len() + 1);
        for location in std::iter::once(BrowseLocation::Folder(ROOT_DIR_ID)).chain(above.iter().map(|segment| segment.id)) {
            let BrowseLocation::Folder(id) = location else { continue };
            ancestors.push((location, self.folder_children(&[id.to_string()], &tree.counts, &options)?));
        }
        Ok(ancestors)
    }

    pub fn browse_directory_path(&self, directory_id: &DirId) -> Result<Vec<library_model::BrowsePathSegment>, DatabaseError> {
        Ok(self.dir_path_components(directory_id)?.into_iter().map(|component| library_model::BrowsePathSegment { id: BrowseLocation::Folder(component.id), name: component.name }).collect())
    }

    /// Children of every folder in `scope`: the direct children normally, and
    /// every folder below when a search widened the scope to the subtree.
    fn folder_children(&self, scope: &[String], counts: &FolderCounts, options: &super::BrowseOptions) -> Result<Vec<BrowseRow>, DatabaseError> {
        let mut folders = Vec::new();
        let mut parents = Vec::new();
        self.connection.folder_children(&json(scope)?, |row| {
            let name: String = row.get(2)?;
            if book_model::normalize_search_text(&name).contains(&options.search) {
                folders.push({
                    let id: String = row.get(1)?;
                    BrowseRow { id: BrowseLocation::Folder(parse_uuid_column(id, 0)?), name, path: None, book_count: 0, downloaded_book_count: 0 }
                });
                parents.push(row.get::<_, String>(0)?);
            }
            Ok(())
        })?;
        // Search results come from anywhere below, so each says where it lives.
        if !options.search.is_empty() {
            let mut paths = crate::placements::FolderPaths::new(&self.connection);
            for (folder, parent) in folders.iter_mut().zip(parents) {
                folder.path = paths.folder(&parent)?.filter(|path| !path.is_empty());
            }
        }
        {
            let folders: &mut Vec<BrowseRow> = &mut folders;
            for folder in folders.iter_mut() {
                let (total, downloaded) = match folder.id {
                    BrowseLocation::Folder(id) => counts.get(&id.to_string()).copied().unwrap_or_default(),
                    BrowseLocation::Subject(_) | BrowseLocation::Author(_) | BrowseLocation::Home(_) | BrowseLocation::Books | BrowseLocation::Catalog(_) | BrowseLocation::Opds { .. } => (0, 0),
                };
                folder.book_count = if options.downloaded_only { downloaded } else { total };
                folder.downloaded_book_count = downloaded;
            }
            let unfiltered = options.search.is_empty() && options.file_types == 0 && options.languages.is_empty() && !options.downloaded_only;
            folders.retain(|folder| unfiltered || folder.book_count > 0);
            // Use the same normalized alphabetical tie-breaker as other browse pages.
            folders.sort_by_cached_key(|folder| (book_model::normalize_search_text(&folder.name), folder.name.clone(), folder.id));
            if options.chip_sort == library_model::BrowseChipSort::BookCount {
                folders.sort_by_key(|folder| std::cmp::Reverse(folder.book_count));
            }
        };
        Ok(folders)
    }

    /// Walk the live folders below `root` one level per query, then count
    /// them all in one query and add each folder's subtree up from the leaves.
    fn folder_tree(&self, root: &str, options: &super::BrowseOptions) -> Result<FolderTree, DatabaseError> {
        let mut folders = vec![root.to_owned()];
        let mut seen = HashSet::from([root.to_owned()]);
        let mut children = HashMap::<String, Vec<String>>::new();
        let mut level = 0..1;
        while !level.is_empty() {
            let parents = json(&folders[level.clone()])?;
            level = level.end..level.end;
            self.connection.folder_children(&parents, |row| {
                let (parent, child) = (row.get::<_, String>(0)?, row.get::<_, String>(1)?);
                // Each folder is visited once, so an ancestry loop cannot walk forever.
                if seen.insert(child.clone()) {
                    children.entry(parent).or_default().push(child.clone());
                    folders.push(child);
                }
                Ok(())
            })?;
            level.end = folders.len();
        }
        let mut counts = FolderCounts::new();
        self.connection.folder_direct_counts(&json(&folders)?, options.file_types, &options.languages, |row| {
            counts.insert(row.get(0)?, (count_column(row, 1)?, count_column(row, 2)?));
            Ok(())
        })?;
        // Children follow their parents in `folders`, so going in reverse
        // completes every subtree before its parent adds it.
        for folder in folders.iter().rev() {
            let below = children.get(folder).into_iter().flatten().map(|child| counts.get(child).copied().unwrap_or_default());
            let (total, downloaded) = below.fold((0, 0), |(total, downloaded), (child_total, child_downloaded)| (total + child_total, downloaded + child_downloaded));
            let entry = counts.entry(folder.clone()).or_default();
            (entry.0, entry.1) = (entry.0 + total, entry.1 + downloaded);
        }
        Ok(FolderTree { folders, counts })
    }
}

/// Books in each folder and everything below it: (all, downloaded).
type FolderCounts = HashMap<String, (usize, usize)>;

/// Every live folder from a page's folder down, and their book counts.
struct FolderTree {
    /// The page's folder first, every folder before its children.
    folders: Vec<String>,
    counts: FolderCounts,
}

impl FolderTree {
    /// Where the page looks for chips and books: its own folder, or every
    /// folder below it when searching.
    fn scope(&self, options: &super::BrowseOptions) -> &[String] {
        if options.search.is_empty() {
            &self.folders[..1]
        } else {
            &self.folders
        }
    }
}

fn count_column(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<usize> {
    usize::try_from(row.get::<_, i64>(column)?).map_err(|_| invalid_integer_column(column, "folder count must not be negative"))
}

fn json(ids: &[impl serde::Serialize]) -> Result<String, DatabaseError> {
    serde_json::to_string(ids).map_err(DatabaseError::operation)
}
