use std::sync::Arc;
use std::time::Duration;

use epub_svelte_lib::library::{AuthorFolderBooks, AuthorSummary, Book, Library};
use gpui::Context;

use crate::BrowserView;
use crate::model::{AuthorGroup, BrowserSection, HomeSection};

struct HomeQuery {
    recently_read: Vec<Book>,
    recently_added: Vec<Book>,
}

struct AuthorsQuery {
    authors: Vec<AuthorSummary>,
    groups: Option<Vec<AuthorFolderBooks>>,
}

struct FoldersQuery {
    directories: Vec<crate::FolderCrumb>,
    books: Vec<Book>,
}

impl BrowserView {
    pub(crate) fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_scheduled {
            return;
        }
        self.refresh_scheduled = true;
        let timer = cx.background_executor().timer(Duration::from_millis(50));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                this.refresh_scheduled = false;
                this.refresh_active_section(cx);
            });
        })
        .detach();
    }

    pub(crate) fn set_library(&mut self, library_id: String, cx: &mut Context<Self>) {
        self.active_library = (self.resolve_library)(&library_id);
        self.active_library_id = Some(library_id);
        self.folder_path.clear();
        self.selected_author = None;
        self.search_active = false;
        self.cover_cache.clear();
        self.cover_order.clear();
        self.cover_costs.clear();
        self.cover_cache_bytes = 0;
        self.missing_cover_ids.clear();
        self.refresh_active_section(cx);
        self.persist(cx);
    }

    pub(crate) fn set_section(&mut self, section: BrowserSection, cx: &mut Context<Self>) {
        if self.section == section {
            return;
        }
        self.section = section;
        self.search_active = false;
        self.selected_author = None;
        self.refresh_active_section(cx);
        self.persist(cx);
    }

    pub(crate) fn set_folder_path(&mut self, folder_path: Vec<crate::FolderCrumb>, cx: &mut Context<Self>) {
        let path_changed = self.folder_path != folder_path;
        self.folder_path = folder_path;
        self.section = BrowserSection::Folders;
        self.search_active = false;
        if path_changed {
            self.load_folder_books(cx);
            self.persist(cx);
        }
        cx.notify();
    }

    pub(crate) fn select_author(&mut self, author: String, cx: &mut Context<Self>) {
        let Some(library) = self.active_library.clone() else {
            return;
        };
        let generation = self.begin_data_load();
        self.selected_author = Some(author.clone());
        self.author_groups = Arc::new(Vec::new());
        cx.notify();

        let query = self.blocking_worker.dispatch(move || library.try_get_author_folder_books_raw(&author).map_err(|error| error.to_string()));
        self.data_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = query.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.data_generation != generation {
                    return;
                }
                match result {
                    Ok(groups) => this.set_author_groups(groups, cx),
                    Err(error) => {
                        log::error!("failed to load author books: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn show_all_authors(&mut self, cx: &mut Context<Self>) {
        self.begin_data_load();
        self.selected_author = None;
        self.author_groups = Arc::new(Vec::new());
        cx.notify();
    }

    pub(crate) fn search(&mut self, query: String, cx: &mut Context<Self>) {
        let query = query.trim().to_owned();
        self.section = BrowserSection::Search;
        if query.chars().count() < 2 {
            self.begin_data_load();
            self.search_active = false;
            self.clear_all_content();
            cx.notify();
            return;
        }
        let Some(library) = self.active_library.clone() else {
            return;
        };

        let generation = self.begin_data_load();
        self.search_active = true;
        self.clear_non_book_content();
        self.books = Arc::new(Vec::new());
        cx.notify();

        let search = self.blocking_worker.dispatch(move || library.try_search_books_raw(&query, 500).map_err(|error| error.to_string()));
        self.data_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = search.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.data_generation != generation {
                    return;
                }
                match result {
                    Ok(books) => this.set_books(books, cx),
                    Err(error) => {
                        log::error!("failed to search books: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn persist(&self, cx: &mut Context<Self>) {
        (self.save_state)(self.session(), None, cx);
    }

    pub(crate) fn refresh_active_section(&mut self, cx: &mut Context<Self>) {
        if matches!(self.section, BrowserSection::Search | BrowserSection::Sync | BrowserSection::Settings) {
            self.begin_data_load();
            if self.section == BrowserSection::Search && !self.search_active {
                self.clear_all_content();
            }
            cx.notify();
            return;
        }
        let Some(library) = self.active_library.clone() else {
            self.begin_data_load();
            self.clear_all_content();
            cx.notify();
            return;
        };

        match self.section {
            BrowserSection::Home => self.load_home(library, cx),
            BrowserSection::Authors => self.load_authors(library, cx),
            BrowserSection::Folders => self.load_folders(library, cx),
            BrowserSection::Search | BrowserSection::Sync | BrowserSection::Settings => unreachable!("static browser sections do not load library data"),
        }
    }

    fn load_home(&mut self, library: Arc<Library>, cx: &mut Context<Self>) {
        let generation = self.begin_data_load();
        self.clear_non_home_content();
        self.home_sections = Arc::new(Vec::new());
        cx.notify();

        let query = self.blocking_worker.dispatch(move || {
            let recently_read = library.try_get_recently_read_raw(20).map_err(|error| error.to_string())?;
            let recently_added = library.try_get_recently_added_raw(20).map_err(|error| error.to_string())?;
            Ok::<_, String>(HomeQuery { recently_read, recently_added })
        });
        self.data_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = query.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.data_generation != generation {
                    return;
                }
                match result {
                    Ok(data) => this.install_home(data, cx),
                    Err(error) => {
                        log::error!("failed to load Home section: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    fn install_home(&mut self, mut data: HomeQuery, cx: &mut Context<Self>) {
        let mut in_progress = Vec::new();
        let mut finished = Vec::new();
        for book in data.recently_read.drain(..) {
            if book.read_progress().value() >= 98.0 {
                finished.push(book);
            } else {
                in_progress.push(book);
            }
        }
        self.set_home_sections(vec![("Recently read", in_progress, true), ("Finished", finished, true), ("Recently added", data.recently_added, false)], cx);
    }

    fn load_authors(&mut self, library: Arc<Library>, cx: &mut Context<Self>) {
        let generation = self.begin_data_load();
        self.clear_non_author_content();
        self.authors = Arc::new(Vec::new());
        let selected_author = self.selected_author.clone();
        cx.notify();

        let query = self.blocking_worker.dispatch(move || {
            let mut authors = library.try_get_authors().map_err(|error| error.to_string())?;
            authors.sort_by_cached_key(|author| author.name().as_str().to_lowercase());
            let groups = match selected_author {
                Some(author) => Some(library.try_get_author_folder_books_raw(&author).map_err(|error| error.to_string())?),
                None => None,
            };
            Ok::<_, String>(AuthorsQuery { authors, groups })
        });
        self.data_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = query.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.data_generation != generation {
                    return;
                }
                match result {
                    Ok(data) => {
                        this.authors = Arc::new(data.authors);
                        if let Some(groups) = data.groups {
                            this.set_author_groups(groups, cx);
                        }
                    }
                    Err(error) => {
                        log::error!("failed to load Authors section: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    fn load_folders(&mut self, library: Arc<Library>, cx: &mut Context<Self>) {
        let generation = self.begin_data_load();
        self.clear_non_folder_content();
        self.folder_directories = Arc::new(Vec::new());
        self.books = Arc::new(Vec::new());
        let folder_id = self.active_folder_id();
        cx.notify();

        let query = self.blocking_worker.dispatch(move || {
            let content = library.try_get_dir_contents_raw(&folder_id).map_err(|error| error.to_string())?;
            let directories = content.dirs.into_iter().map(|directory| crate::FolderCrumb { id: directory.id().as_str().to_owned(), name: directory.name().as_str().to_owned(), book_count: directory.book_count() }).collect();
            Ok::<_, String>(FoldersQuery { directories, books: content.books })
        });
        self.data_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = query.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.data_generation != generation {
                    return;
                }
                match result {
                    Ok(data) => {
                        this.folder_directories = Arc::new(data.directories);
                        this.set_books(data.books, cx);
                    }
                    Err(error) => {
                        log::error!("failed to load Folders section: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    fn load_folder_books(&mut self, cx: &mut Context<Self>) {
        let Some(library) = self.active_library.clone() else {
            return;
        };
        let generation = self.begin_data_load();
        self.folder_directories = Arc::new(Vec::new());
        self.books = Arc::new(Vec::new());
        let folder_id = self.active_folder_id();
        cx.notify();

        let query = self.blocking_worker.dispatch(move || library.try_get_dir_contents_raw(&folder_id).map_err(|error| error.to_string()));
        self.data_task = Some(cx.spawn(async move |this, cx| {
            let Ok(result) = query.recv().await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.data_generation != generation {
                    return;
                }
                match result {
                    Ok(content) => {
                        this.folder_directories =
                            Arc::new(content.dirs.into_iter().map(|directory| crate::FolderCrumb { id: directory.id().as_str().to_owned(), name: directory.name().as_str().to_owned(), book_count: directory.book_count() }).collect());
                        this.set_books(content.books, cx);
                    }
                    Err(error) => {
                        log::error!("failed to load folder contents: {error}");
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn active_folder_id(&self) -> String {
        self.folder_path.last().map(|crumb| crumb.id.clone()).unwrap_or_else(|| "__root__".to_owned())
    }

    fn begin_data_load(&mut self) -> u64 {
        self.data_task.take();
        self.data_generation = self.data_generation.wrapping_add(1);
        self.advance_cover_generation();
        self.data_generation
    }

    pub(crate) fn set_books(&mut self, books: Vec<Book>, _cx: &mut Context<Self>) {
        let cards = self.cards_from_books(books, false);
        self.books = Arc::new(cards);
    }

    fn set_home_sections(&mut self, sections: Vec<(&'static str, Vec<Book>, bool)>, cx: &mut Context<Self>) {
        let sections = sections.into_iter().filter(|(_, books, _)| !books.is_empty()).map(|(title, books, show_progress)| HomeSection { title: title.into(), books: Arc::new(self.cards_from_books(books, show_progress)) }).collect();
        self.home_sections = Arc::new(sections);
        let visible_ids = self.home_sections.iter().flat_map(|section| section.books.iter()).filter(|card| card.cover.is_none()).map(|card| card.id).collect::<Vec<_>>();
        self.queue_cover_load(visible_ids, cx);
    }

    pub(crate) fn set_author_groups(&mut self, mut groups: Vec<AuthorFolderBooks>, _cx: &mut Context<Self>) {
        groups.sort_by(|left, right| compare_folder_hierarchy(left.folder_path().as_str(), right.folder_path().as_str()));
        let groups = groups
            .into_iter()
            .map(|group| {
                let mut books = group.books().to_vec();
                books.sort_by_cached_key(|book| book.title().as_str().to_lowercase());
                AuthorGroup { folder_path: group.folder_path().as_str().to_owned().into(), books: Arc::new(self.cards_from_books(books, false)) }
            })
            .collect();
        self.author_groups = Arc::new(groups);
    }

    fn clear_all_content(&mut self) {
        self.folder_directories = Arc::new(Vec::new());
        self.home_sections = Arc::new(Vec::new());
        self.authors = Arc::new(Vec::new());
        self.author_groups = Arc::new(Vec::new());
        self.books = Arc::new(Vec::new());
    }

    fn clear_non_home_content(&mut self) {
        self.folder_directories = Arc::new(Vec::new());
        self.authors = Arc::new(Vec::new());
        self.author_groups = Arc::new(Vec::new());
        self.books = Arc::new(Vec::new());
    }

    fn clear_non_author_content(&mut self) {
        self.folder_directories = Arc::new(Vec::new());
        self.home_sections = Arc::new(Vec::new());
        self.books = Arc::new(Vec::new());
        self.author_groups = Arc::new(Vec::new());
    }

    fn clear_non_folder_content(&mut self) {
        self.home_sections = Arc::new(Vec::new());
        self.authors = Arc::new(Vec::new());
        self.author_groups = Arc::new(Vec::new());
    }

    fn clear_non_book_content(&mut self) {
        self.folder_directories = Arc::new(Vec::new());
        self.home_sections = Arc::new(Vec::new());
        self.authors = Arc::new(Vec::new());
        self.author_groups = Arc::new(Vec::new());
    }
}

fn compare_folder_hierarchy(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = left.split('/').collect::<Vec<_>>();
    let right_parts = right.split('/').collect::<Vec<_>>();
    for (left_part, right_part) in left_parts.iter().zip(right_parts.iter()) {
        let ordering = left_part.to_lowercase().cmp(&right_part.to_lowercase());
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    left_parts.len().cmp(&right_parts.len()).then_with(|| left.to_lowercase().cmp(&right.to_lowercase()))
}
