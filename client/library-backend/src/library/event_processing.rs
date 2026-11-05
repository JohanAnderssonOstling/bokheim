//! Apply queued events under the library state lock and reconcile contents updates.

use super::events::LibraryEvent;
use super::LibraryResources;
use crate::BackendError;
use library_model::LibraryUpdate;
use library_placement::LibraryState;

impl LibraryResources {
    pub(super) async fn publish_book_file_activity(&self) {
        match self.book_file_activity().await {
            Ok(activity) => self.events.updates.publish(LibraryUpdate::BookFileActivity(activity)),
            Err(error) => log::warn!("could not refresh library file activity: {error}"),
        }
    }
    pub(crate) async fn drain_events(&self, storage: &LibraryState) {
        let events = self.event_state.lock().expect("library events").receiver();
        let mut contents_changed = false;
        let mut refresh_contents = false;
        let mut refresh_activity = false;
        while let Ok(event) = events.try_recv() {
            match event {
                LibraryEvent::ContentsChanged => contents_changed = true,
                LibraryEvent::RefreshContents => refresh_contents = true,
                LibraryEvent::RefreshFileActivity => refresh_activity = true,
                event => self.apply_event(storage, event).await,
            }
        }
        if contents_changed || refresh_contents {
            match storage.database().text_search_trash_updates().await {
                Ok(updates) => {
                    for update in updates {
                        self.events.updates.publish(LibraryUpdate::TextSearchRemoved(update));
                    }
                }
                Err(error) => log::warn!("could not remove trashed books from search snapshots: {error}"),
            }
            let event = if contents_changed { LibraryEvent::ContentsChanged } else { LibraryEvent::RefreshContents };
            self.event_state.lock().expect("library events").apply(event, &self.events);
        }
        if contents_changed || refresh_contents || refresh_activity {
            self.publish_book_file_activity().await;
        }
    }
    async fn apply_event(&self, storage: &LibraryState, event: LibraryEvent) {
        if let LibraryEvent::ThumbnailFinished { book_id, result } = event {
            let apply = async {
                if storage.database().thumbnail_book_format(book_id).await?.is_none() {
                    return Ok::<(), BackendError>(());
                }
                match result {
                    Ok(has_cover) => {
                        if storage.database().thumbnail_work_state(&book_id).await?.as_deref() == Some("pending") {
                            if let Some(has_cover) = has_cover {
                                storage.database().complete_thumbnail_work(&book_id, has_cover).await?;
                                if has_cover {
                                    self.events.cover_changed(book_id);
                                }
                            } else {
                                storage.database().discard_thumbnail_work(&book_id).await?;
                            }
                            self.events.enrichment.wake();
                            self.events.sync.wake();
                        }
                    }
                    Err(failure) => self.events.updates.publish(LibraryUpdate::ThumbnailFailed { book_id, failure }),
                }
                Ok(())
            };
            if let Err(error) = apply.await {
                log::warn!("could not apply thumbnail notification: {error}");
            }
            return;
        }
        self.event_state.lock().expect("library events").apply(event, &self.events);
    }
}
