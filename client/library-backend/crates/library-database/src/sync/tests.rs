//! Sync concern tests: apply, state, and outbox.

#[cfg(test)]
mod apply_tests {
    use crate::*;
    use library_replica::{BookDetails, LibraryStateEntry, ReadingPositionState, StatePublication};
    use sync_common::{LibraryRevision, PullStateResponse, ReplicaSeq, SyncCursor};

    use crate::test_support::TestDatabase;

    fn open_test_database() -> TestDatabase {
        TestDatabase::new("sync-apply")
    }


    fn placement_change(hash: sync_common::ContentHash, changed_at: u64) -> sync_common::StateDelivery {
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-333333333333").unwrap();
        let body = LibraryStateEntry::Placement { dir_id, content_hash: hash, present: true, origin_folder_id: None };
        let mutation = body.publication(sync_common::ReplicaId::nil(), changed_at, ReplicaSeq::new(1).unwrap());
        sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap())
    }

    fn reading_change(hash: sync_common::ContentHash, cfi: &str, progress: f32, changed_at: u64) -> sync_common::StateDelivery {
        let body = LibraryStateEntry::ReadingPosition { content_hash: hash, value: ReadingPositionState { location: book_model::ReadingPosition::parse(cfi).unwrap(), progress } };
        let mutation = body.publication(sync_common::ReplicaId::nil(), changed_at, ReplicaSeq::new(1).unwrap());
        sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap())
    }

    fn metadata_change(hash: sync_common::ContentHash, title: &str, author: &str, changed_at: u64) -> sync_common::StateDelivery {
        let body = LibraryStateEntry::Metadata {
            content_hash: hash,
            value: BookDetails { title: title.to_owned(), subtitle: None, contributors: vec![book_model::Contributor::new(author, book_model::MarcRelatorCode(*b"aut")).unwrap()], book: book_model::BookMetadata::default() },
        };
        let mutation = body.publication(sync_common::ReplicaId::nil(), changed_at, ReplicaSeq::new(1).unwrap());
        sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap())
    }

    #[test]
    fn pull_commit_applies_reading_advances_cursor_and_holds_stale() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"7".repeat(64));
        let next = SyncCursor { lifecycle_revision: None, field_revision: Some(LibraryRevision::new(3).unwrap()) };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), reading_change(hash, "epubcfi(/6/4)", 0.5, 100)], next_cursor: next.clone(), has_more: false };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("commit reading page"));
        assert_eq!(database.sync_pull_cursor().expect("read cursor"), next);
        assert_eq!(
            ({
                use crate::browse::rows::{invalid_integer_column, invalid_text_column};
                use crate::browse::DetailSql;
                use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
                use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
                use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
                use sync_common::ContentHash;
                struct SourceLocation {
                    source_directory_id: Option<String>,
                    path: Vec<library_model::BrowsePathSegment>,
                }
                let database: &Database = &database;
                let content_hash = hash;

                database.with_browse_snapshot(|| {
                    let (book, audiobook_chapter_count) = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut book = None;
                        database.connection.book_detail_book(content_hash.as_str(), |row| {
                            let chapter_count: Option<i64> = row.get(1)?;
                            let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                            book = Some((row.get::<_, i64>(0)?, chapter_count));
                            Ok(())
                        })?;
                        let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                        let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                        (card, chapter_count)
                    };
                    let description = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut description = String::new();
                        database.connection.book_detail_description(content_hash.as_str(), |row| {
                            description = row.get(0)?;
                            Ok(())
                        })?;
                        description
                    };
                    let SourceLocation { source_directory_id, path } = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut source_directory_id: Option<String> = None;
                        database.connection.book_detail_directory(content_hash.as_str(), |row| {
                            source_directory_id = row.get(0)?;
                            Ok(())
                        })?;

                        let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                            .as_deref()
                            .map(|directory| {
                                let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                                database.dir_path_components(&directory)
                            })
                            .transpose()?
                            .unwrap_or_default()
                            .into_iter()
                            .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                            .collect();

                        SourceLocation { source_directory_id, path }
                    };
                    let authors = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut authors = Vec::new();
                        database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                            let id: String = row.get(0)?;
                            authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                            Ok(())
                        })?;

                        authors
                    };
                    let credits = {
                        let content_hash: &ContentHash = &content_hash;
                        // Only narrator and translator: a reader picks an audiobook by
                        // its narrator and an edition by its translator, and nothing else in the
                        // contributor list answers a question a reader is asking.
                        let mut credits = Vec::new();
                        let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                        let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                        database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                            let id: String = row.get(0)?;
                            let role: Vec<u8> = row.get(2)?;
                            credits.push(library_model::BookDetailCredit {
                                id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                                name: row.get(1)?,
                                role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                            });
                            Ok(())
                        })?;

                        credits
                    };
                    let toc = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut stored: Option<String> = None;
                        database.connection.book_toc_select(content_hash.as_str(), |row| {
                            stored = row.get(0)?;
                            Ok(())
                        })?;
                        let mut entries = stored
                            .map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}"))))
                            .transpose()?
                            .unwrap_or_default();
                        for (index, entry) in entries.iter_mut().enumerate() {
                            if book_model::audiobook_toc_position(&entry.target).is_some() {
                                entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                            }
                        }
                        entries
                    };
                    let (current_entry, current_target) = {
                        let content_hash: &ContentHash = &content_hash;
                        let format: library_model::LibraryFileTypeFilter = book.format_category;
                        let toc: &[library_model::BookTocEntry] = &toc;
                        'pull_commit_applies_reading_advances_cursor_and_holds_stale: {
                            if format == library_model::LibraryFileTypeFilter::Audiobook {
                                let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'pull_commit_applies_reading_advances_cursor_and_holds_stale (None, None) };
                                let chapter = ({
                                    use crate::{chapters_from_navigation, AudiobookSql};
                                    use library_model::{AudiobookChapter, BookTocEntry};
                                    let hash: &sync_common::ContentHash = content_hash;

                                    'chapter: {
                                        let mut stored: Option<(String, i64)> = None;
                                        database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                            stored = Some((row.get(0)?, row.get(1)?));
                                            Ok(())
                                        })?;
                                        let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                        let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                        chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                                    }
                                })
                                .into_iter()
                                .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                                break 'pull_commit_applies_reading_advances_cursor_and_holds_stale match chapter {
                                    Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                                    None => (None, None),
                                };
                            }
                            let mut target: Option<String> = None;
                            database.connection.reading_entry_select(content_hash.as_str(), |row| {
                                target = row.get(0)?;
                                Ok(())
                            })?;
                            let Some(target) = target else { break 'pull_commit_applies_reading_advances_cursor_and_holds_stale (None, None) };
                            (entry_title(toc, &target), Some(target))
                        }
                    };
                    Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
                })
            })
            .expect("read detail")
            .book
            .progress,
            0.5
        );
        let stale = PullStateResponse { mutations: vec![reading_change(hash, "epubcfi(/6/2)", 0.9, 50)], next_cursor: next.clone(), has_more: false };
        assert!(database.commit_existing_books_fixture(&[], &stale).expect("commit stale page"));
        assert_eq!(
            ({
                use crate::browse::rows::{invalid_integer_column, invalid_text_column};
                use crate::browse::DetailSql;
                use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
                use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
                use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
                use sync_common::ContentHash;
                struct SourceLocation {
                    source_directory_id: Option<String>,
                    path: Vec<library_model::BrowsePathSegment>,
                }
                let database: &Database = &database;
                let content_hash = hash;

                database.with_browse_snapshot(|| {
                    let (book, audiobook_chapter_count) = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut book = None;
                        database.connection.book_detail_book(content_hash.as_str(), |row| {
                            let chapter_count: Option<i64> = row.get(1)?;
                            let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                            book = Some((row.get::<_, i64>(0)?, chapter_count));
                            Ok(())
                        })?;
                        let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                        let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                        (card, chapter_count)
                    };
                    let description = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut description = String::new();
                        database.connection.book_detail_description(content_hash.as_str(), |row| {
                            description = row.get(0)?;
                            Ok(())
                        })?;
                        description
                    };
                    let SourceLocation { source_directory_id, path } = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut source_directory_id: Option<String> = None;
                        database.connection.book_detail_directory(content_hash.as_str(), |row| {
                            source_directory_id = row.get(0)?;
                            Ok(())
                        })?;

                        let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                            .as_deref()
                            .map(|directory| {
                                let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                                database.dir_path_components(&directory)
                            })
                            .transpose()?
                            .unwrap_or_default()
                            .into_iter()
                            .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                            .collect();

                        SourceLocation { source_directory_id, path }
                    };
                    let authors = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut authors = Vec::new();
                        database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                            let id: String = row.get(0)?;
                            authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                            Ok(())
                        })?;

                        authors
                    };
                    let credits = {
                        let content_hash: &ContentHash = &content_hash;
                        // Only narrator and translator: a reader picks an audiobook by
                        // its narrator and an edition by its translator, and nothing else in the
                        // contributor list answers a question a reader is asking.
                        let mut credits = Vec::new();
                        let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                        let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                        database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                            let id: String = row.get(0)?;
                            let role: Vec<u8> = row.get(2)?;
                            credits.push(library_model::BookDetailCredit {
                                id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                                name: row.get(1)?,
                                role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                            });
                            Ok(())
                        })?;

                        credits
                    };
                    let toc = {
                        let content_hash: &ContentHash = &content_hash;
                        let mut stored: Option<String> = None;
                        database.connection.book_toc_select(content_hash.as_str(), |row| {
                            stored = row.get(0)?;
                            Ok(())
                        })?;
                        let mut entries = stored
                            .map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}"))))
                            .transpose()?
                            .unwrap_or_default();
                        for (index, entry) in entries.iter_mut().enumerate() {
                            if book_model::audiobook_toc_position(&entry.target).is_some() {
                                entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                            }
                        }
                        entries
                    };
                    let (current_entry, current_target) = {
                        let content_hash: &ContentHash = &content_hash;
                        let format: library_model::LibraryFileTypeFilter = book.format_category;
                        let toc: &[library_model::BookTocEntry] = &toc;
                        'pull_commit_applies_reading_advances_cursor_and_holds_stale: {
                            if format == library_model::LibraryFileTypeFilter::Audiobook {
                                let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'pull_commit_applies_reading_advances_cursor_and_holds_stale (None, None) };
                                let chapter = ({
                                    use crate::{chapters_from_navigation, AudiobookSql};
                                    use library_model::{AudiobookChapter, BookTocEntry};
                                    let hash: &sync_common::ContentHash = content_hash;

                                    'chapter: {
                                        let mut stored: Option<(String, i64)> = None;
                                        database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                            stored = Some((row.get(0)?, row.get(1)?));
                                            Ok(())
                                        })?;
                                        let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                        let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                        chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                                    }
                                })
                                .into_iter()
                                .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                                break 'pull_commit_applies_reading_advances_cursor_and_holds_stale match chapter {
                                    Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                                    None => (None, None),
                                };
                            }
                            let mut target: Option<String> = None;
                            database.connection.reading_entry_select(content_hash.as_str(), |row| {
                                target = row.get(0)?;
                                Ok(())
                            })?;
                            let Some(target) = target else { break 'pull_commit_applies_reading_advances_cursor_and_holds_stale (None, None) };
                            (entry_title(toc, &target), Some(target))
                        }
                    };
                    Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
                })
            })
            .expect("reread detail")
            .book
            .progress,
            0.5
        );
    }

    fn toc_change(hash: sync_common::ContentHash, changed_at: u64) -> sync_common::StateDelivery {
        let body = LibraryStateEntry::BookToc {
            content_hash: hash,
            value: book_model::BookTocDocument {
                entries: vec![
                    book_model::BookTocEntry { title: "Opening".into(), target: book_model::audiobook_toc_target(0), children: Vec::new() },
                    book_model::BookTocEntry { title: "Conclusion".into(), target: book_model::audiobook_toc_target(400), children: Vec::new() },
                ],
                duration_ms: Some(1000),
                tracks: Some(vec![
                    book_model::AudiobookTrack { name: "01.mp3".into(), start_ms: 0, end_ms: 400, offset: 40, length: 100, archive_checksum: None },
                    book_model::AudiobookTrack { name: "02.mp3".into(), start_ms: 400, end_ms: 1000, offset: 180, length: 120, archive_checksum: None },
                ]),
            },
        };
        batch_mutation("444444444444".to_owned(), body, changed_at, 7)
    }

    #[test]
    fn pull_commit_applies_audiobook_navigation_with_duration() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"9".repeat(64));
        let next = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(5).unwrap()), field_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), toc_change(hash, 100)], next_cursor: next, has_more: false };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("commit navigation page"));
        // The last chapter runs to the end of the audio: without the synced
        // duration the chapters would not resolve at all.
        let chapters = database.audiobook_chapters(&hash).expect("read applied chapters");
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[1].end_ms, 1000);
        assert_eq!(database.audiobook_tracks(&hash).unwrap().iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["01.mp3", "02.mp3"]);
        // Remote state must not echo back into the push outbox.
        assert!(database.sync_publishable_mutations().expect("read outbox").iter().all(|mutation| !matches!(&mutation.state.value, LibraryStateEntry::BookToc { .. })));
    }

    #[test]
    fn local_audiobook_tracks_are_in_the_toc_outbox() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        database.connection.execute("INSERT INTO book(content_hash) VALUES(?1)", [hash.as_str()]).unwrap();
        let archive_checksum = sync_common::ContentHash::new(&"b".repeat(64));
        let tracks = vec![book_model::AudiobookTrack { name: "01.mp3".into(), start_ms: 0, end_ms: 1000, offset: 40, length: 100, archive_checksum: Some(archive_checksum) }];
        database.commit_audiobook_metadata(&hash, &book_metadata::AudiobookMetadata { title: "Opening".into(), authors: Vec::new(), description: String::new(), duration_ms: 1000, chapters: vec![book_metadata::AudiobookChapterMetadata { title: "Opening".into(), start_ms: 0, end_ms: 1000 }], tracks: tracks.clone(), chapter_origin: book_model::ChapterOrigin::EmbeddedM4b, recording: Default::default() }).unwrap();
        let outbox = database.sync_publishable_mutations().unwrap();
        let (_, toc) = outbox
            .iter()
            .find_map(|mutation| match &mutation.state.value {
                LibraryStateEntry::BookToc { content_hash, value } if content_hash == &hash => Some((content_hash, value)),
                _ => None,
            })
            .expect("book TOC in outbox");
        assert_eq!(toc.duration_ms, Some(1000));
        assert_eq!(toc.tracks.as_deref(), Some(tracks.as_slice()));
    }

    fn projected_toc(database: &Database, hash: sync_common::ContentHash) -> book_model::BookTocDocument {
        let (entries, count, duration, tracks): (String, i64, Option<i64>, Option<String>) = database
            .connection
            .query_row(
                "SELECT toc_json,entry_count,duration_ms,tracks_json FROM book_navigation WHERE content_hash=?1",
                [hash.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let entries: Vec<book_model::BookTocEntry> = serde_json::from_str(&entries).unwrap();
        assert_eq!(count, crate::sync::toc_entry_count(&entries) as i64);
        book_model::BookTocDocument { entries, duration_ms: duration.map(|value| u64::try_from(value).unwrap()), tracks: tracks.map(|value| serde_json::from_str(&value).unwrap()) }
    }

    #[test]
    fn toc_replacement_converges_across_order_pages_absence_and_replay() {
        let hash = batch_hash(910);
        let other = batch_hash(911);
        let LibraryStateEntry::BookToc { value: full, .. } = LibraryStateEntry::decode(&toc_change(hash, 100).state).unwrap().unwrap() else { unreachable!() };
        let empty = book_model::BookTocDocument::default();
        let duration_only = book_model::BookTocDocument { entries: vec![full.entries[0].clone()], duration_ms: Some(600), tracks: None };
        let mut changed = full.clone();
        changed.entries.truncate(1);
        changed.entries[0].title = "Replacement".into();
        changed.duration_ms = Some(1200);
        changed.tracks = Some(vec![book_model::AudiobookTrack { name: "new.mp3".into(), start_ms: 0, end_ms: 1200, offset: 10, length: 50, archive_checksum: None }]);
        for (old, winner) in [(full.clone(), empty.clone()), (full.clone(), duration_only), (full.clone(), changed), (empty, full.clone())] {
            let old = batch_mutation("910".into(), LibraryStateEntry::BookToc { content_hash: hash, value: old }, 100, 7);
            let newer = batch_mutation("911".into(), LibraryStateEntry::BookToc { content_hash: hash, value: winner.clone() }, 200, 7);
            for changes in [vec![old.clone(), newer.clone()], vec![newer, old]] {
                for page_size in [1, 2] {
                    let holder = open_test_database();
                    let database = &holder.database;
                    apply_metadata_test_pages(database, &[toc_change(other, 100)], 1);
                    apply_metadata_test_pages(database, &changes, page_size);
                    assert_eq!(projected_toc(database, hash), winner);
                    assert_eq!(projected_toc(database, other), full, "replacement is scoped to one book");
                    apply_metadata_test_pages(database, &changes, page_size);
                    assert_eq!(projected_toc(database, hash), winner, "stale updates and replay cannot change the winner");
                    assert!(database.sync_publishable_mutations().unwrap().is_empty(), "projection must not publish new mutations");
                }
            }
        }
    }

    #[test]
    fn toc_replacement_rolls_back_optional_field_removal_if_entries_fail() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = batch_hash(912);
        apply_metadata_test_pages(database, &[toc_change(hash, 100)], 1);
        let before = projected_toc(database, hash);
        let before_version: i64 = database.connection.query_row("SELECT changed_at FROM sync_state_version WHERE state_kind='book_toc' AND state_key=?1", [hash.as_str()], |row| row.get(0)).unwrap();
        let before_cursor = database.sync_pull_cursor().unwrap();
        database.connection.execute_batch("CREATE TEMP TRIGGER reject_toc_replacement BEFORE UPDATE ON book_navigation BEGIN SELECT RAISE(ABORT,'injected TOC failure'); END;").unwrap();
        let page = PullStateResponse {
            mutations: vec![batch_mutation("912".into(), LibraryStateEntry::BookToc { content_hash: hash, value: book_model::BookTocDocument::default() }, 200, 7)],
            next_cursor: SyncCursor { lifecycle_revision: Some(LibraryRevision::new(2).unwrap()), field_revision: None },
            has_more: false,
        };
        assert!(database.commit_existing_books_fixture(&[], &page).is_err());
        assert_eq!(projected_toc(database, hash), before);
        assert_eq!(database.sync_pull_cursor().unwrap(), before_cursor);
        assert_eq!(database.connection.query_row("SELECT changed_at FROM sync_state_version WHERE state_kind='book_toc' AND state_key=?1", [hash.as_str()], |row| row.get::<_, i64>(0)).unwrap(), before_version);
        database.connection.execute_batch("DROP TRIGGER reject_toc_replacement;").unwrap();
        database.commit_existing_books_fixture(&[], &page).unwrap();
        assert_eq!(projected_toc(database, hash), book_model::BookTocDocument::default());
    }

    #[test]
    fn pull_commit_projects_metadata_identity_and_holds_stale() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"8".repeat(64));
        let next = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(5).unwrap()), field_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), metadata_change(hash, "Remote Title", "Remote Author", 100)], next_cursor: next.clone(), has_more: false };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("commit metadata page"));
        let detail = ({
            use crate::browse::rows::{invalid_integer_column, invalid_text_column};
            use crate::browse::DetailSql;
            use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
            use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
            use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
            use sync_common::ContentHash;
            struct SourceLocation {
                source_directory_id: Option<String>,
                path: Vec<library_model::BrowsePathSegment>,
            }
            let database: &Database = &database;
            let content_hash = hash;

            database.with_browse_snapshot(|| {
                let (book, audiobook_chapter_count) = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut book = None;
                    database.connection.book_detail_book(content_hash.as_str(), |row| {
                        let chapter_count: Option<i64> = row.get(1)?;
                        let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                        book = Some((row.get::<_, i64>(0)?, chapter_count));
                        Ok(())
                    })?;
                    let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    (card, chapter_count)
                };
                let description = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut description = String::new();
                    database.connection.book_detail_description(content_hash.as_str(), |row| {
                        description = row.get(0)?;
                        Ok(())
                    })?;
                    description
                };
                let SourceLocation { source_directory_id, path } = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut source_directory_id: Option<String> = None;
                    database.connection.book_detail_directory(content_hash.as_str(), |row| {
                        source_directory_id = row.get(0)?;
                        Ok(())
                    })?;

                    let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                        .as_deref()
                        .map(|directory| {
                            let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                            database.dir_path_components(&directory)
                        })
                        .transpose()?
                        .unwrap_or_default()
                        .into_iter()
                        .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                        .collect();

                    SourceLocation { source_directory_id, path }
                };
                let authors = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut authors = Vec::new();
                    database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                        let id: String = row.get(0)?;
                        authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                        Ok(())
                    })?;

                    authors
                };
                let credits = {
                    let content_hash: &ContentHash = &content_hash;
                    // Only narrator and translator: a reader picks an audiobook by
                    // its narrator and an edition by its translator, and nothing else in the
                    // contributor list answers a question a reader is asking.
                    let mut credits = Vec::new();
                    let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                    let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                    database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                        let id: String = row.get(0)?;
                        let role: Vec<u8> = row.get(2)?;
                        credits.push(library_model::BookDetailCredit {
                            id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                            name: row.get(1)?,
                            role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                        });
                        Ok(())
                    })?;

                    credits
                };
                let toc = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut stored: Option<String> = None;
                    database.connection.book_toc_select(content_hash.as_str(), |row| {
                        stored = row.get(0)?;
                        Ok(())
                    })?;
                    let mut entries = stored
                        .map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}"))))
                        .transpose()?
                        .unwrap_or_default();
                    for (index, entry) in entries.iter_mut().enumerate() {
                        if book_model::audiobook_toc_position(&entry.target).is_some() {
                            entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                        }
                    }
                    entries
                };
                let (current_entry, current_target) = {
                    let content_hash: &ContentHash = &content_hash;
                    let format: library_model::LibraryFileTypeFilter = book.format_category;
                    let toc: &[library_model::BookTocEntry] = &toc;
                    'detail: {
                        if format == library_model::LibraryFileTypeFilter::Audiobook {
                            let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'detail (None, None) };
                            let chapter = ({
                                use crate::{chapters_from_navigation, AudiobookSql};
                                use library_model::{AudiobookChapter, BookTocEntry};
                                let hash: &sync_common::ContentHash = content_hash;

                                'chapter: {
                                    let mut stored: Option<(String, i64)> = None;
                                    database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                        stored = Some((row.get(0)?, row.get(1)?));
                                        Ok(())
                                    })?;
                                    let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                    let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                    chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                                }
                            })
                            .into_iter()
                            .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                            break 'detail match chapter {
                                Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                                None => (None, None),
                            };
                        }
                        let mut target: Option<String> = None;
                        database.connection.reading_entry_select(content_hash.as_str(), |row| {
                            target = row.get(0)?;
                            Ok(())
                        })?;
                        let Some(target) = target else { break 'detail (None, None) };
                        (entry_title(toc, &target), Some(target))
                    }
                };
                Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
            })
        })
        .expect("read detail");
        assert_eq!(detail.book.title, "Remote Title");
        assert!(detail.authors.iter().any(|author| author.name == "Remote Author"), "incoming contributor is projected");
        let stale = PullStateResponse { mutations: vec![metadata_change(hash, "Stale Title", "Stale Author", 50)], next_cursor: next.clone(), has_more: false };
        assert!(database.commit_existing_books_fixture(&[], &stale).expect("commit stale page"));
        let detail = ({
            use crate::browse::rows::{invalid_integer_column, invalid_text_column};
            use crate::browse::DetailSql;
            use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
            use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
            use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
            use sync_common::ContentHash;
            struct SourceLocation {
                source_directory_id: Option<String>,
                path: Vec<library_model::BrowsePathSegment>,
            }
            let database: &Database = &database;
            let content_hash = hash;

            database.with_browse_snapshot(|| {
                let (book, audiobook_chapter_count) = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut book = None;
                    database.connection.book_detail_book(content_hash.as_str(), |row| {
                        let chapter_count: Option<i64> = row.get(1)?;
                        let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                        book = Some((row.get::<_, i64>(0)?, chapter_count));
                        Ok(())
                    })?;
                    let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    (card, chapter_count)
                };
                let description = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut description = String::new();
                    database.connection.book_detail_description(content_hash.as_str(), |row| {
                        description = row.get(0)?;
                        Ok(())
                    })?;
                    description
                };
                let SourceLocation { source_directory_id, path } = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut source_directory_id: Option<String> = None;
                    database.connection.book_detail_directory(content_hash.as_str(), |row| {
                        source_directory_id = row.get(0)?;
                        Ok(())
                    })?;

                    let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                        .as_deref()
                        .map(|directory| {
                            let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                            database.dir_path_components(&directory)
                        })
                        .transpose()?
                        .unwrap_or_default()
                        .into_iter()
                        .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                        .collect();

                    SourceLocation { source_directory_id, path }
                };
                let authors = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut authors = Vec::new();
                    database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                        let id: String = row.get(0)?;
                        authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                        Ok(())
                    })?;

                    authors
                };
                let credits = {
                    let content_hash: &ContentHash = &content_hash;
                    // Only narrator and translator: a reader picks an audiobook by
                    // its narrator and an edition by its translator, and nothing else in the
                    // contributor list answers a question a reader is asking.
                    let mut credits = Vec::new();
                    let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                    let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                    database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                        let id: String = row.get(0)?;
                        let role: Vec<u8> = row.get(2)?;
                        credits.push(library_model::BookDetailCredit {
                            id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                            name: row.get(1)?,
                            role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                        });
                        Ok(())
                    })?;

                    credits
                };
                let toc = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut stored: Option<String> = None;
                    database.connection.book_toc_select(content_hash.as_str(), |row| {
                        stored = row.get(0)?;
                        Ok(())
                    })?;
                    let mut entries = stored
                        .map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}"))))
                        .transpose()?
                        .unwrap_or_default();
                    for (index, entry) in entries.iter_mut().enumerate() {
                        if book_model::audiobook_toc_position(&entry.target).is_some() {
                            entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                        }
                    }
                    entries
                };
                let (current_entry, current_target) = {
                    let content_hash: &ContentHash = &content_hash;
                    let format: library_model::LibraryFileTypeFilter = book.format_category;
                    let toc: &[library_model::BookTocEntry] = &toc;
                    'detail: {
                        if format == library_model::LibraryFileTypeFilter::Audiobook {
                            let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'detail (None, None) };
                            let chapter = ({
                                use crate::{chapters_from_navigation, AudiobookSql};
                                use library_model::{AudiobookChapter, BookTocEntry};
                                let hash: &sync_common::ContentHash = content_hash;

                                'chapter: {
                                    let mut stored: Option<(String, i64)> = None;
                                    database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                        stored = Some((row.get(0)?, row.get(1)?));
                                        Ok(())
                                    })?;
                                    let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                    let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                    chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                                }
                            })
                            .into_iter()
                            .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                            break 'detail match chapter {
                                Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                                None => (None, None),
                            };
                        }
                        let mut target: Option<String> = None;
                        database.connection.reading_entry_select(content_hash.as_str(), |row| {
                            target = row.get(0)?;
                            Ok(())
                        })?;
                        let Some(target) = target else { break 'detail (None, None) };
                        (entry_title(toc, &target), Some(target))
                    }
                };
                Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
            })
        })
        .expect("reread detail");
        assert_eq!(detail.book.title, "Remote Title");
        assert!(detail.authors.iter().any(|author| author.name == "Remote Author"), "stale contributor never lands");
    }

    fn batch_hash(index: u64) -> sync_common::ContentHash {
        sync_common::ContentHash::new(&format!("{index:064x}"))
    }

    fn apply_metadata_test_pages(database: &Database, changes: &[sync_common::StateDelivery], page_size: usize) {
        for chunk in changes.chunks(page_size) {
            let page = PullStateResponse { mutations: chunk.to_vec(), next_cursor: SyncCursor::default(), has_more: false };
            database.commit_existing_books_fixture(&[], &page).unwrap();
        }
    }

    fn assert_metadata_projection(database: &Database, hash: sync_common::ContentHash, expected: &BookDetails) {
        let (title, subtitle): (String, Option<String>) =
            database.connection.query_row("SELECT title,subtitle FROM book WHERE content_hash=?1", [hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(title, expected.title);
        assert_eq!(subtitle, expected.subtitle);
        let contributors = database
            .connection
            .prepare("SELECT name FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY position")
            .unwrap()
            .query_map([hash.as_str()], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(contributors, expected.contributors.iter().map(|credit| credit.name().to_owned()).collect::<Vec<_>>());
    }

    #[test]
    fn metadata_register_owns_projection_across_arrival_orders_and_page_boundaries() {
        let hash = batch_hash(901);
        let canonical = batch_metadata(901);
        let metadata = batch_mutation("901".into(), LibraryStateEntry::Metadata { content_hash: hash, value: canonical.clone() }, 10, 1);
        let addition = batch_mutation("902".into(), LibraryStateEntry::BookLifecycle { content_hash: hash, value: library_replica::BookLifecycleState::Present }, 20, 1);
        for changes in [vec![metadata.clone(), addition.clone()], vec![addition, metadata]] {
            for page_size in [1, 2] {
                let holder = open_test_database();
                let database = &holder.database;
                apply_metadata_test_pages(database, &changes, page_size);
                assert_metadata_projection(database, hash, &canonical);
                crate::sync::convergence_tests::acknowledge_confirmed_reconciliation(database, &changes);
                // Replayed values cannot alter either register's projection.
                apply_metadata_test_pages(database, &changes, page_size);
                assert_metadata_projection(database, hash, &canonical);
                assert!(database.sync_publishable_mutations().unwrap().is_empty());
            }
        }
    }

    #[test]
    fn lifecycle_alone_does_not_materialize_metadata() {
        let holder = open_test_database();
        let database = &holder.database;
        apply_metadata_test_pages(database, &[addition_change(903, 10), addition_change(903, 20)], 1);
        let (title, subtitle, contributors): (Option<String>, Option<String>, i64) = database
            .connection
            .query_row("SELECT title,subtitle,(SELECT COUNT(*) FROM book_contributor WHERE book_row_id=book.row_id) FROM book WHERE content_hash=?1", [batch_hash(903).as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!((title, subtitle, contributors), (None, None, 0));
    }

    fn batch_metadata(index: u64) -> BookDetails {
        BookDetails {
            title: format!("Batch Title {index}"),
            subtitle: None,
            contributors: vec![book_model::Contributor::new(format!("Batch Author {index}"), book_model::MarcRelatorCode(*b"aut")).unwrap()],
            book: book_model::BookMetadata::default(),
        }
    }

    fn batch_mutation(tag: String, body: LibraryStateEntry, changed_at: u64, replica: u128) -> sync_common::StateDelivery {
        let mutation = body.publication(sync_common::ReplicaId::nil(), changed_at, ReplicaSeq::new(1).unwrap());
        sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::from_u128(replica), LibraryRevision::new(1).unwrap())
    }

    fn addition_change(index: u64, changed_at: u64) -> sync_common::StateDelivery {
        let body = LibraryStateEntry::BookLifecycle { content_hash: batch_hash(index), value: library_replica::BookLifecycleState::Present };
        batch_mutation(format!("{index:012}"), body, changed_at, u128::from(index))
    }

    fn removal_change(hash: sync_common::ContentHash, tag: &str, changed_at: u64, purged: bool) -> sync_common::StateDelivery {
        let value = if purged { library_replica::BookLifecycleState::Purged } else { library_replica::BookLifecycleState::Deleted { origin_folder_id: None } };
        batch_mutation(tag.to_owned(), LibraryStateEntry::BookLifecycle { content_hash: hash, value }, changed_at, 9_999_999)
    }

    fn state_snapshot(database: &Database) -> (Vec<(String, Option<String>, bool)>, Vec<(String, String, String, i64, i64, i64)>, Vec<String>, Vec<(String, String, i64)>, SyncCursor) {
        let books = database
            .connection
            .prepare("SELECT content_hash, title, deleted_at IS NOT NULL FROM book ORDER BY content_hash")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let versions = database
            .connection
            .prepare("SELECT state_kind, state_key, state_subkey, changed_at, conflict_rank, replica_seq FROM sync_state_version ORDER BY state_kind, state_key, state_subkey")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let work = database.connection.prepare("SELECT content_hash FROM local_book_work ORDER BY content_hash").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<Vec<String>, _>>().unwrap();
        let contributors = database
            .connection
            .prepare("SELECT b.content_hash, c.name, c.position FROM book_contributor c JOIN book b ON b.row_id = c.book_row_id ORDER BY b.content_hash, c.position")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (books, versions, work, contributors, database.sync_pull_cursor().expect("read cursor"))
    }

    fn decoy_hash() -> sync_common::ContentHash {
        sync_common::ContentHash::new(&"f".repeat(64))
    }

    #[test]
    fn batched_additions_match_ordered_across_bounded_snapshots() {
        let batched_holder = open_test_database();
        let ordered_holder = open_test_database();
        let batched = &batched_holder.database;
        let ordered = &ordered_holder.database;
        // Seeds land identically through the ordered path on both sides.
        // Index 200 is newer than the page, 201 older, 202 deleted, 203
        // purged, 204 owned by a newer metadata change.
        let seed = vec![
            addition_change(200, 110),
            addition_change(201, 10),
            addition_change(202, 10),
            removal_change(batch_hash(202), "000000000900", 20, false),
            addition_change(203, 10),
            removal_change(batch_hash(203), "000000000901", 20, true),
            addition_change(204, 10),
            metadata_change(batch_hash(204), "Metadata owns this title", "Metadata Author", 120),
            placement_change(decoy_hash(), 90),
        ];
        let next_seed = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(2).unwrap()), field_revision: None };
        for database in [batched, ordered] {
            let page = PullStateResponse { mutations: seed.clone(), next_cursor: next_seed.clone(), has_more: true };
            assert!(database.commit_existing_books_fixture(&[], &page).expect("commit seed page"));
        }
        let mut incoming: Vec<sync_common::StateDelivery> = (0..205).map(|index| addition_change(index, 100)).collect();
        let next = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(3).unwrap()), field_revision: None };
        let page = PullStateResponse { mutations: incoming.clone(), next_cursor: next.clone(), has_more: false };
        assert!(batched.commit_existing_books_fixture(&[], &page).expect("commit batched page"));
        // One stale change forces the whole page onto the ordered path but
        // loses its version check, so it must not perturb the outcome.
        incoming.push(placement_change(decoy_hash(), 50));
        let page = PullStateResponse { mutations: incoming, next_cursor: next.clone(), has_more: false };
        assert!(ordered.commit_existing_books_fixture(&[], &page).expect("commit ordered page"));
        assert_eq!(state_snapshot(batched), state_snapshot(ordered));
        let (books, _, _, _, _) = state_snapshot(batched);
        let title = |index: u64| books.iter().find(|(hash, _, _)| *hash == batch_hash(index).as_str()).map(|(_, title, _)| title.clone());
        assert_eq!(title(200), Some(None), "lifecycle has no metadata");
        assert_eq!(title(201), Some(None));
        assert_eq!(title(204), Some(Some("Metadata owns this title".to_owned())), "metadata-owned title survives the top-up");
        assert_eq!(title(203), Some(None), "lifecycle revival does not recreate metadata");
        assert!(books.iter().any(|(hash, _, deleted)| *hash == batch_hash(203).as_str() && !deleted), "revived book is not purged");
    }

    #[test]
    fn batched_metadata_matches_ordered_for_missing_and_existing_books() {
        let batched_holder = open_test_database();
        let ordered_holder = open_test_database();
        let batched = &batched_holder.database;
        let ordered = &ordered_holder.database;
        // Even indexes exist up front; odd indexes arrive through ensure.
        // Index 100 is newer than the page, 101 purged, 102 carries a title
        // that a metadata change must still overwrite.
        let mut seed = Vec::new();
        for index in (0..100).step_by(2) {
            seed.push(placement_change(batch_hash(index), 90));
        }
        seed.push(metadata_change(batch_hash(100), "Newer owns this title", "Newer Author", 110));
        seed.push(addition_change(101, 10));
        seed.push(removal_change(batch_hash(101), "000000000902", 20, true));
        seed.push(addition_change(102, 10));
        seed.push(placement_change(decoy_hash(), 90));
        let next_seed = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(2).unwrap()), field_revision: None };
        for database in [batched, ordered] {
            let page = PullStateResponse { mutations: seed.clone(), next_cursor: next_seed.clone(), has_more: true };
            assert!(database.commit_existing_books_fixture(&[], &page).expect("commit seed page"));
        }
        let mut incoming: Vec<sync_common::StateDelivery> = (0..103).map(|index| metadata_change(batch_hash(index), &format!("Batch Title {index}"), &format!("Meta Author {index}"), 100)).collect();
        let next = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(3).unwrap()), field_revision: None };
        let page = PullStateResponse { mutations: incoming.clone(), next_cursor: next.clone(), has_more: false };
        assert!(batched.commit_existing_books_fixture(&[], &page).expect("commit batched page"));
        incoming.push(placement_change(decoy_hash(), 50));
        let page = PullStateResponse { mutations: incoming, next_cursor: next.clone(), has_more: false };
        assert!(ordered.commit_existing_books_fixture(&[], &page).expect("commit ordered page"));
        assert_eq!(state_snapshot(batched), state_snapshot(ordered));
        let (books, _, _, contributors, _) = state_snapshot(batched);
        let title = |index: u64| books.iter().find(|(hash, _, _)| *hash == batch_hash(index).as_str()).map(|(_, title, _)| title.clone());
        assert_eq!(title(102), Some(Some("Batch Title 102".to_owned())));
        assert!(contributors.iter().any(|(hash, name, _)| *hash == batch_hash(102).as_str() && *name == "Meta Author 102"), "metadata overwrites despite stored title");
        assert_eq!(title(1), Some(Some("Batch Title 1".to_owned())), "missing book arrives through ensure");
        assert_eq!(title(100), Some(Some("Newer owns this title".to_owned())));
        assert!(!books.iter().any(|(hash, _, _)| *hash == batch_hash(101).as_str()), "purged book must not be recreated by metadata");
    }

    #[test]
    fn batches_flush_when_chunk_and_page_end_with_stale_changes() {
        for metadata in [false, true] {
            let batched_holder = open_test_database();
            let ordered_holder = open_test_database();
            let change = |index, timestamp| {
                if metadata {
                    metadata_change(batch_hash(index), &format!("Title {index}"), "Author", timestamp)
                } else {
                    addition_change(index, timestamp)
                }
            };
            let mut seed: Vec<_> = [99, 199, 204].into_iter().map(|index| change(index, 200)).collect();
            seed.push(placement_change(decoy_hash(), 90));
            let seed_page = PullStateResponse { mutations: seed, next_cursor: SyncCursor::default(), has_more: true };
            for database in [&batched_holder.database, &ordered_holder.database] {
                database.commit_existing_books_fixture(&[], &seed_page).unwrap();
            }
            let mut page = PullStateResponse {
                mutations: (0..205).map(|index| change(index, 100)).collect(),
                next_cursor: SyncCursor { lifecycle_revision: Some(LibraryRevision::new(3).unwrap()), field_revision: None },
                has_more: false,
            };
            batched_holder.database.commit_existing_books_fixture(&[], &page).unwrap();
            // A stale placement selects the ordered path without changing state.
            page.mutations.push(placement_change(decoy_hash(), 50));
            ordered_holder.database.commit_existing_books_fixture(&[], &page).unwrap();
            assert_eq!(state_snapshot(&batched_holder.database), state_snapshot(&ordered_holder.database), "metadata={metadata}");
            // Replaying an entirely stale batch must preserve the same state.
            page.mutations.pop();
            let before = state_snapshot(&batched_holder.database);
            batched_holder.database.commit_existing_books_fixture(&[], &page).unwrap();
            assert_eq!(before, state_snapshot(&batched_holder.database));
        }
    }

    #[test]
    fn failed_pull_rolls_back_acknowledgements_versions_and_projections() {
        for mode in 0..3 {
            let holder = open_test_database();
            let database = &holder.database;
            database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Local folder".to_owned()).unwrap();
            let pending_ids = || database.sync_publishable_mutations().unwrap().iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>();
            let ids = pending_ids();
            assert!(!ids.is_empty());
            let before = state_snapshot(database);
            let mutations = match mode {
                0 => vec![addition_change(1, 100)],
                1 => vec![metadata_change(batch_hash(1), "Title", "Author", 100)],
                _ => vec![addition_change(1, 100), reading_change(batch_hash(1), "epubcfi(/6/2!/4/2/1:0)", 0.5, 101)],
            };
            let page =
                PullStateResponse { mutations, next_cursor: SyncCursor { lifecycle_revision: Some(LibraryRevision::new(3).unwrap()), field_revision: Some(LibraryRevision::new(4).unwrap()) }, has_more: false };
            // Fail after domain writes, when the transaction advances its cursor.
            database.connection.execute_batch("CREATE TEMP TRIGGER reject_pull_cursor BEFORE UPDATE OF last_pull_lifecycle_seq ON sync_metadata BEGIN SELECT RAISE(ABORT, 'injected pull failure'); END;").unwrap();
            assert!(database.commit_existing_books_fixture(&ids, &page).is_err());
            assert_eq!(before, state_snapshot(database));
            assert_eq!(ids, pending_ids());
            database.connection.execute_batch("DROP TRIGGER reject_pull_cursor;").unwrap();
            assert!(database.commit_existing_books_fixture(&ids, &page).unwrap());
            assert!(pending_ids().is_empty(), "remote changes must not be echoed into the outbox");
            assert_eq!(page.next_cursor, database.sync_pull_cursor().unwrap());
        }
    }

    #[test]
    fn empty_pull_acknowledges_without_reporting_remote_changes() {
        let holder = open_test_database();
        let database = &holder.database;
        database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Local folder".to_owned()).unwrap();
        let ids: Vec<_> = database.sync_publishable_mutations().unwrap().iter().map(|mutation| mutation.publisher_seq).collect();
        assert!(!ids.is_empty());
        let mut page = PullStateResponse { mutations: Vec::new(), next_cursor: database.sync_pull_cursor().unwrap(), has_more: false };
        assert!(!database.commit_existing_books_fixture(&ids, &page).unwrap());
        assert!(database.sync_publishable_mutations().unwrap().is_empty());
        assert!(!database.commit_existing_books_fixture(&[], &page).unwrap());
        page.next_cursor.lifecycle_revision = Some(LibraryRevision::new(3).unwrap());
        assert!(!database.commit_existing_books_fixture(&[], &page).unwrap());
        assert_eq!(page.next_cursor, database.sync_pull_cursor().unwrap());
    }

    fn directory_change(body: LibraryStateEntry, changed_at: u64, tag: &str) -> sync_common::StateDelivery {
        let mutation = body.publication(sync_common::ReplicaId::nil(), changed_at, ReplicaSeq::new(1).unwrap());
        sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap())
    }

    #[test]
    fn pull_commit_applies_directory_intents_and_projection() {
        let holder = open_test_database();
        let database = &holder.database;
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-444444444444").unwrap();
        let next = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(6).unwrap()), field_revision: None };
        let page = PullStateResponse {
            mutations: vec![
                directory_change(LibraryStateEntry::DirectoryName { dir_id, name: "Shelf".to_owned() }, 10, "444444444441"),
                directory_change(LibraryStateEntry::DirectoryParent { dir_id, parent_id: sync_common::ROOT_DIR_ID }, 20, "444444444442"),
                directory_change(LibraryStateEntry::DirectoryLifecycle { dir_id, value: library_replica::DirectoryLifecycleState::Present }, 30, "444444444443"),
            ],
            next_cursor: next.clone(),
            has_more: false,
        };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("commit directory page"));
        let row: (String, String, i64, String, String) = database
            .connection
            .prepare("SELECT canonical_name, canonical_parent_id, lifecycle, name, parent_id FROM dir WHERE id = ?1")
            .unwrap()
            .query_row([dir_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .unwrap();
        assert_eq!(row.0, "Shelf");
        assert_eq!(row.1, sync_common::ROOT_DIR_ID.to_string());
        assert_eq!(row.2, 0);
        assert_eq!(row.3, "Shelf", "projected name follows the intent");
        assert_eq!(row.4, sync_common::ROOT_DIR_ID.to_string());
    }

    #[test]
    fn pull_commit_stores_description() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"9".repeat(64));
        let body = LibraryStateEntry::Description { content_hash: hash, value: "A\x00noisy\x07 description.".to_owned() };
        let mutation = body.publication(sync_common::ReplicaId::nil(), 100, ReplicaSeq::new(1).unwrap());
        let change = sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap());
        let next = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(7).unwrap()), field_revision: None };
        let page = PullStateResponse { mutations: vec![change], next_cursor: next, has_more: false };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("commit description page"));
        let stored: String = database.connection.prepare("SELECT d.description FROM book_description d JOIN book b ON b.row_id = d.book_row_id WHERE b.content_hash = ?1").unwrap().query_row([hash.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(stored, "Anoisy description.");
    }

    #[test]
    fn pull_commit_projects_subject_classification() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        let mut book = book_model::BookMetadata::default();
        book.subjects = vec![book_model::BookSubject::new(None, "Distributed systems", "dc:subject", None, None).unwrap()];
        let body = LibraryStateEntry::Metadata {
            content_hash: hash,
            value: BookDetails { title: "Classified".to_owned(), subtitle: None, contributors: vec![book_model::Contributor::new("Subject Author", book_model::MarcRelatorCode(*b"aut")).unwrap()], book },
        };
        let mutation = body.publication(sync_common::ReplicaId::nil(), 100, ReplicaSeq::new(1).unwrap());
        let change = sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap());
        let next = SyncCursor { lifecycle_revision: Some(LibraryRevision::new(8).unwrap()), field_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), change], next_cursor: next, has_more: false };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("commit classified page"));
        let subjects: Vec<String> = database
            .connection
            .prepare("SELECT s.name FROM book_subject s JOIN book b ON b.row_id = s.book_row_id WHERE b.content_hash = ?1 ORDER BY s.position")
            .unwrap()
            .query_map([hash.as_str()], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(subjects.iter().any(|subject| subject.contains("Distributed systems")), "subject survives projection, got {subjects:?}");
    }

    #[test]
    fn languages_project_the_complete_winning_value() {
        use crate::metadata::write_metadata;
        use crate::sync::apply::project_book;
        use book_model::LanguageTag;

        let holder = open_test_database();
        let database = &holder.database;
        let hash = "e".repeat(64);
        let tx = database.connection.unchecked_transaction().expect("open tx");
        tx.execute("INSERT INTO book(content_hash, added_at, format) VALUES(?1, 1, 1)", [hash.as_str()]).expect("seed book");
        let read_languages = |tx: &rusqlite::Transaction| -> Vec<String> {
            tx.prepare("SELECT language FROM book_language WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY language")
                .expect("prepare")
                .query_map([hash.as_str()], |row| row.get(0))
                .expect("query")
                .collect::<Result<Vec<String>, _>>()
                .expect("collect")
        };
        let mut first = book_model::BookMetadata::default();
        first.languages = vec![LanguageTag::parse("en").expect("tag")];
        write_metadata(&tx, &hash, "Book", None, &[], project_book(&first).unwrap()).expect("project");
        assert_eq!(read_languages(&tx), vec!["en".to_owned()]);
        // A later winner replaces the complete language list.
        let mut second = first.clone();
        second.languages = vec![LanguageTag::parse("fr").expect("tag")];
        write_metadata(&tx, &hash, "Book", None, &[], project_book(&second).unwrap()).expect("reproject");
        assert_eq!(read_languages(&tx), vec!["fr".to_owned()]);
        second.languages.clear();
        write_metadata(&tx, &hash, "Book", None, &[], project_book(&second).unwrap()).unwrap();
        assert!(read_languages(&tx).is_empty());
    }

    #[test]
    fn publishers_are_not_projected() {
        use crate::metadata::write_metadata;
        use crate::sync::apply::project_book;

        let holder = open_test_database();
        let database = &holder.database;
        let hash = "f".repeat(64);
        let tx = database.connection.unchecked_transaction().expect("open tx");
        tx.execute("INSERT INTO book(content_hash, added_at, format) VALUES(?1, 1, 1)", [hash.as_str()]).expect("seed book");
        let mut book = book_model::BookMetadata::default();
        book.publishers = vec![book_model::PublisherCredit::with_id(book_model::agent_id_from_migration_seed(b"test-publisher"), "Acme Press").expect("credit")];
        write_metadata(&tx, &hash, "Book", None, &[], project_book(&book).unwrap()).expect("project");
        let tables: i64 = tx.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('publisher', 'book_publisher')", [], |row| row.get(0)).expect("count");
        assert_eq!(tables, 0);
    }
}

#[cfg(test)]
mod state_tests {
    use sync_common::{StateAddress, SyncCursor};

    use crate::test_support::TestDatabase;

    fn open_test_database() -> TestDatabase {
        TestDatabase::new("sync-state")
    }

    #[test]
    fn recovery_flag_lifecycle() {
        let holder = open_test_database();
        let database = &holder.database;
        assert!(!database.sync_cursor_recovery_pending().expect("read flag"));
        database.sync_begin_cursor_recovery().expect("begin recovery");
        assert!(database.sync_cursor_recovery_pending().expect("reread flag"));
        assert_eq!(database.sync_pull_cursor().expect("read cursor"), SyncCursor::default());
        // No version rows exist, so the first page is also the last and clears
        // the marker.
        assert_eq!(database.sync_recover_state_page(None).expect("recover page"), (None, 0));
        assert!(!database.sync_cursor_recovery_pending().expect("cleared flag"));
    }

    #[test]
    fn missing_cells_reencode_current_state() {
        let holder = open_test_database();
        let database = &holder.database;
        let created = database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Repair seed".to_owned()).expect("seed directory");
        let ids: Vec<sync_common::ReplicaSeq> = database.sync_publishable_mutations().expect("read snapshot").iter().map(|mutation| mutation.publisher_seq).collect();
        database.sync_acknowledge_mutations(&ids).expect("acknowledge");
        let cell = StateAddress { kind: "directory_name".to_owned(), entity_key: created.id.to_string(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[cell]).expect("enqueue"), 1);
        // Already queued cells are skipped without re-encoding.
        let snapshot = database.sync_publishable_mutations().expect("reread snapshot");
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(snapshot[0].state.value, library_replica::LibraryStateEntry::DirectoryName { .. }));
        let duplicate = StateAddress { kind: "directory_name".to_owned(), entity_key: created.id.to_string(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[duplicate]).expect("re-enqueue"), 0);
        // Unknown kinds have no current value and are skipped.
        let unknown = StateAddress { kind: "no_such_kind".to_owned(), entity_key: "x".to_owned(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[unknown]).expect("enqueue unknown"), 0);
    }
}

#[cfg(test)]
mod outbox_tests {
    use sync_common::ReplicaSeq;

    use crate::test_support::TestDatabase;

    fn open_test_database() -> TestDatabase {
        TestDatabase::new("sync-outbox")
    }

    #[test]
    fn publishable_snapshot_round_trips_through_acknowledgement() {
        let holder = open_test_database();
        let database = &holder.database;
        database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Ack seed".to_owned()).expect("seed directory");
        let snapshot = database.sync_publishable_mutations().expect("read snapshot");
        assert_eq!(snapshot.len(), 3);
        let ids: Vec<ReplicaSeq> = snapshot.iter().map(|mutation| mutation.publisher_seq).collect();
        database.sync_acknowledge_mutations(&ids).expect("acknowledge");
        assert!(database.sync_publishable_mutations().expect("reread snapshot").is_empty());
        database.sync_acknowledge_mutations(&[]).expect("empty acknowledgement is a no-op");
    }

    #[test]
    fn acknowledgement_commits_caller_sized_batches() {
        let holder = open_test_database();
        let database = &holder.database;
        for index in 0..90 {
            database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &format!("Batch {index}")).expect("seed directory");
        }
        let snapshot = database.sync_publishable_mutations().expect("read snapshot");
        assert_eq!(snapshot.len(), 270);
        let ids: Vec<ReplicaSeq> = snapshot.iter().map(|mutation| mutation.publisher_seq).collect();
        for chunk in ids.chunks(256) {
            database.sync_acknowledge_mutations(chunk).expect("acknowledge chunk");
        }
        assert!(database.sync_publishable_mutations().expect("reread snapshot").is_empty());
    }
}

mod annotation_tests {
    use crate::*;
    use book_model::{AnnotationAnchor, AnnotationStyle, ReaderAnnotation};
    use library_replica::{LibraryStateEntry, StatePublication};
    use sync_common::{LibraryRevision, ReplicaSeq, SyncCursor};

    use crate::test_support::TestDatabase;

    fn open_test_database() -> TestDatabase {
        TestDatabase::new("annotations")
    }

    fn seed_book(database: &Database, hash: sync_common::ContentHash) {
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-444444444444").unwrap();
        let body = LibraryStateEntry::Placement { dir_id, content_hash: hash, present: true, origin_folder_id: None };
        let mutation = body.publication(sync_common::ReplicaId::nil(), 90, ReplicaSeq::new(1).unwrap());
        let page = sync_common::PullStateResponse {
            mutations: vec![sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap())],
            next_cursor: SyncCursor { lifecycle_revision: Some(LibraryRevision::new(2).unwrap()), field_revision: None },
            has_more: false,
        };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("seed book"));
    }

    fn annotation(id: &str, hash: book_model::ContentHash, ordinal: Option<i64>, progress: Option<f32>, modified_at: i64, note: &str) -> ReaderAnnotation {
        ReaderAnnotation {
            id: id.to_owned(),
            state: book_model::AnnotationState {
                content_hash: hash,
                detail: book_model::AnnotationDetail {
                    anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4,/1:0,/1:4)"),
                    exact_text: "Text".to_owned(),
                    style: AnnotationStyle::Highlight,
                    color: "#f6c945".to_owned(),
                    note: note.to_owned(),
                    created_at: 10,
                    toc_ordinal: ordinal,
                    progress: progress,
                },
                modified_at: u64::try_from(modified_at).expect("annotation timestamps must be nonnegative"),
                deleted: false,
            },
        }
    }

    fn annotation_mutations(database: &Database) -> Vec<StatePublication> {
        database.sync_publishable_mutations().expect("read snapshot").into_iter().filter(|mutation| matches!(mutation.state.value, LibraryStateEntry::Annotation { .. })).collect()
    }

    #[test]
    fn stored_annotations_round_trip_detail_and_order_by_reading_position() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        seed_book(database, hash);
        let hash = book_model::ContentHash::new(&"a".repeat(64));
        database.upsert_annotation(&annotation("third", hash.clone(), None, None, 30, "ungrouped")).expect("store unpositioned");
        database.upsert_annotation(&annotation("first", hash.clone(), Some(2), Some(10.0), 20, "early")).expect("store early");
        database.upsert_annotation(&annotation("second", hash.clone(), Some(5), Some(60.0), 10, "late")).expect("store late");
        let stored = database.annotations(sync_common::ContentHash::new(&"a".repeat(64))).expect("read annotations");
        assert_eq!(stored.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["first", "second", "third"]);
        assert_eq!(stored[0].note, "early");
        assert_eq!(stored[0].toc_ordinal, Some(2));
        assert_eq!(stored[0].progress, Some(10.0));
        assert_eq!(stored[0].anchor, AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4,/1:0,/1:4)"));
        assert_eq!(stored[0].created_at, 10);
    }

    #[test]
    fn reupsert_preserves_created_at_and_updates_detail() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"b".repeat(64));
        seed_book(database, hash);
        let hash = book_model::ContentHash::new(&"b".repeat(64));
        database.upsert_annotation(&annotation("one", hash.clone(), Some(1), Some(5.0), 10, "old")).expect("store");
        let mut edited = annotation("one", hash, Some(1), Some(5.0), 20, "new");
        edited.created_at = 99;
        database.upsert_annotation(&edited).expect("restore");
        let stored = database.annotations(sync_common::ContentHash::new(&"b".repeat(64))).expect("read annotations");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].note, "new");
        assert_eq!(stored[0].created_at, 10, "edits must not move creation time");
        assert_eq!(stored[0].modified_at, 20);
    }

    #[test]
    fn delete_hides_row_and_publishes_tombstone() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"c".repeat(64));
        seed_book(database, hash);
        let hash = book_model::ContentHash::new(&"c".repeat(64));
        database.upsert_annotation(&annotation("one", hash, Some(3), Some(25.0), 10, "note")).expect("store");
        assert_eq!(annotation_mutations(database).len(), 1);
        database.sync_acknowledge_mutations(&annotation_mutations(database).iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>()).expect("acknowledge");
        database.delete_annotation("one", 11).expect("delete");
        assert!(database.annotations(sync_common::ContentHash::new(&"c".repeat(64))).expect("read annotations").is_empty());
        let tombstones = annotation_mutations(database);
        assert_eq!(tombstones.len(), 1);
        let LibraryStateEntry::Annotation { annotation_id, value } = &tombstones[0].state.value else { panic!("expected annotation mutation") };
        assert_eq!(annotation_id, "one");
        assert!(value.deleted);
        assert_eq!(value.note, "note");
        assert_eq!(value.toc_ordinal, Some(3));
    }

    #[test]
    fn delete_unknown_id_is_an_error() {
        let holder = open_test_database();
        let database = &holder.database;
        assert!(database.delete_annotation("missing", 11).is_err());
    }

    #[test]
    fn remote_annotation_apply_stores_positioned_detail() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"d".repeat(64));
        let body = LibraryStateEntry::Annotation {
            annotation_id: "remote".to_owned(),
            value: book_model::AnnotationState {
                content_hash: book_model::ContentHash::new(&"d".repeat(64)),
                detail: book_model::AnnotationDetail {
                    anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
                    exact_text: "Remote".to_owned(),
                    style: AnnotationStyle::Underline,
                    color: "#fff".to_owned(),
                    note: "remote note".to_owned(),
                    created_at: 5,
                    toc_ordinal: Some(7),
                    progress: Some(80.0),
                },
                modified_at: 15,
                deleted: false,
            },
        };
        let mutation = body.publication(sync_common::ReplicaId::nil(), 100, ReplicaSeq::new(1).unwrap());
        let page = sync_common::PullStateResponse {
            mutations: vec![sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&mutation).unwrap(), uuid::Uuid::new_v4(), LibraryRevision::new(1).unwrap())],
            next_cursor: SyncCursor { lifecycle_revision: Some(LibraryRevision::new(3).unwrap()), field_revision: None },
            has_more: false,
        };
        assert!(database.commit_existing_books_fixture(&[], &page).expect("commit annotation page"));
        let stored = database.annotations(hash).expect("read annotations");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].note, "remote note");
        assert_eq!(stored[0].toc_ordinal, Some(7));
        assert_eq!(stored[0].progress, Some(80.0));
        assert_eq!(stored[0].created_at, 5);
    }
}
