use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use browser_ui::NavigationSession;
use gpui::{App, Bounds, Context, Pixels, Task, point, px, size};
use reader_ui::ReaderPreferences;
use serde::{Deserialize, Serialize};

use crate::blocking_worker::BlockingWorker;

const SESSION_VERSION: u32 = 1;

fn default_kobo_auto_rotate() -> bool {
    true
}

/// Geometry for the one application window.
///
/// This used to describe a separate reader window. Keeping the same serialized
/// shape lets existing desktop sessions migrate without losing their layout.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WindowGeometry {
    pub fn from_bounds(bounds: Bounds<Pixels>) -> Self {
        Self { x: f32::from(bounds.origin.x), y: f32::from(bounds.origin.y), width: f32::from(bounds.size.width), height: f32::from(bounds.size.height) }
    }

    pub fn bounds(self) -> Bounds<Pixels> {
        Bounds { origin: point(px(self.x), px(self.y)), size: size(px(self.width), px(self.height)) }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiSession {
    version: u32,
    #[serde(default = "default_kobo_auto_rotate")]
    pub kobo_auto_rotate: bool,
    pub reader_preferences: ReaderPreferences,
    #[serde(default)]
    pub navigation: NavigationSession,
    #[serde(default, alias = "reader_window")]
    pub window: Option<WindowGeometry>,
}

impl Default for UiSession {
    fn default() -> Self {
        Self { version: SESSION_VERSION, kobo_auto_rotate: default_kobo_auto_rotate(), reader_preferences: ReaderPreferences::default(), navigation: NavigationSession::default(), window: None }
    }
}

impl UiSession {
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::default();
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(session) if session.version == SESSION_VERSION => session,
            Ok(_) => {
                log::warn!("ignoring GPUI session with an unsupported version");
                Self::default()
            }
            Err(error) => {
                log::warn!("failed to read GPUI session: {error}");
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            log::error!("failed to create GPUI session directory: {error}");
            return;
        }
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(error) = atomic_session_write(path, |file| file.write_all(&bytes)) {
                    log::error!("failed to save GPUI session: {error}");
                }
            }
            Err(error) => {
                log::error!("failed to serialize GPUI session: {error}");
            }
        }
    }
}

// Keep the previous session intact until all new bytes are durably written.
// The temporary file must be adjacent so replacement stays on one filesystem.
fn atomic_session_write(path: &Path, write: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::Builder::new().prefix(".gpui-session-").tempfile_in(parent)?;
    write(staged.as_file_mut())?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn session_path(app_data: &Path) -> PathBuf {
    app_data.join("gpui-ui-session.json")
}

/// One UI-thread owner for the saved session and its pending disk writes.
pub struct SessionPersistence {
    pub session: UiSession,
    path: PathBuf,
    deadline: Option<Instant>,
    worker_running: bool,
    save_task: Option<Task<()>>,
    blocking_worker: BlockingWorker,
}

impl SessionPersistence {
    pub fn new(session: UiSession, path: PathBuf, blocking_worker: BlockingWorker) -> Self {
        Self { session, path, deadline: None, worker_running: false, save_task: None, blocking_worker }
    }

    pub fn schedule(&mut self, cx: &mut Context<Self>) {
        self.deadline = Some(cx.background_executor().now() + Duration::from_millis(300));
        if self.worker_running {
            return;
        }
        self.worker_running = true;
        let executor = cx.background_executor().clone();
        self.save_task = Some(cx.spawn(async move |owner, cx| {
            loop {
                let Ok(Some(wait)) = owner.update(cx, |owner, _| owner.deadline.map(|deadline| deadline.saturating_duration_since(executor.now()))) else {
                    break;
                };
                executor.timer(wait).await;
                let Ok(save) = owner.update(cx, |owner, _| {
                    if owner.deadline.is_some_and(|deadline| deadline <= executor.now()) {
                        owner.deadline = None;
                        Some(owner.write_snapshot())
                    } else {
                        None
                    }
                }) else {
                    break;
                };
                if let Some(save) = save {
                    let _ = save.recv().await;
                }
                let finished = owner
                    .update(cx, |owner, _| {
                        if owner.deadline.is_none() {
                            owner.worker_running = false;
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(true);
                if finished {
                    break;
                }
            }
        }));
    }

    fn write_snapshot(&self) -> async_channel::Receiver<()> {
        let session = self.session.clone();
        let path = self.path.clone();
        self.blocking_worker.dispatch(move || session.save(&path))
    }

    /// Navigation changes are queued immediately so recovery does not depend on quitting.
    pub fn save_now(&self, cx: &mut App) {
        let save = self.write_snapshot();
        cx.spawn(async move |_| {
            let _ = save.recv().await;
        })
        .detach();
    }

    pub fn flush(&mut self) {
        self.save_task.take();
        self.deadline = None;
        self.worker_running = false;
        let _ = self.write_snapshot().recv_blocking();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use browser_ui::{GlobalRoute, NavigationSession, Route};
    use gpui::{AppContext, TestAppContext};

    fn saved_navigation() -> NavigationSession {
        serde_json::from_value(serde_json::json!({
            "library_id": "00000000-0000-0000-0000-000000000002",
            "route": {"Library": "Folders"},
            "pages": [{"path": [[{"Folder": "00000000-0000-0000-0000-000000000003"}, "Fiction"]], "detail": "07".repeat(32)}],
            "reader": {"locator": {"library_id": "00000000-0000-0000-0000-000000000002", "content_hash": "07".repeat(32)}, "title": "Book"}
        }))
        .unwrap()
    }

    #[test]
    fn legacy_sessions_keep_reader_preferences_and_default_navigation() {
        let directory = tempfile::tempdir().unwrap();
        let path = session_path(directory.path());
        let mut original = UiSession::default();
        original.reader_preferences.toc_width = 417.0;
        original.window = Some(WindowGeometry { x: 10.0, y: 20.0, width: 900.0, height: 700.0 });
        let mut legacy = serde_json::to_value(&original).unwrap();
        legacy.as_object_mut().unwrap().remove("navigation");
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let loaded = UiSession::load(&path);
        assert_eq!(loaded.reader_preferences, original.reader_preferences);
        assert_eq!(loaded.window, original.window);
        assert_eq!(loaded.navigation, NavigationSession::default());
    }

    #[gpui::test]
    async fn navigation_and_reader_are_saved_without_shutdown_and_closing_reader_stays_closed(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = session_path(directory.path());
        let scheduler = cx.new(|cx| SessionPersistence::new(UiSession::default(), path.clone(), BlockingWorker::new(cx.background_executor(), 1)));
        scheduler.update(cx, |owner, cx| {
            owner.session.navigation = saved_navigation();
            owner.save_now(cx);
        });
        let worker = scheduler.read_with(cx, |owner, _| owner.blocking_worker.clone());
        // A barrier on the same ordered worker waits for the disk write, without a quit or flush.
        worker.dispatch(|| ()).recv().await.unwrap();
        assert_eq!(UiSession::load(&path).navigation, scheduler.read_with(cx, |owner, _| owner.session.navigation.clone()));

        scheduler.update(cx, |owner, cx| {
            owner.session.navigation.reader = None;
            owner.session.navigation.route = Route::Global(GlobalRoute::Settings);
            owner.save_now(cx);
        });
        worker.dispatch(|| ()).recv().await.unwrap();
        let restored = UiSession::load(&path).navigation;
        assert!(restored.reader.is_none());
        assert_eq!(restored.route, Route::Global(GlobalRoute::Settings));
        assert_eq!(restored.library_id, scheduler.read_with(cx, |owner, _| owner.session.navigation.library_id));
    }

    #[gpui::test]
    async fn delayed_saves_coalesce_and_include_immediate_navigation_changes(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = session_path(directory.path());
        let owner = cx.new(|cx| SessionPersistence::new(UiSession::default(), path.clone(), BlockingWorker::new(cx.background_executor(), 1)));
        let worker = owner.read_with(cx, |owner, _| owner.blocking_worker.clone());
        owner.update(cx, |owner, cx| {
            owner.session.reader_preferences.toc_width = 300.0;
            owner.schedule(cx);
        });
        cx.run_until_parked();
        cx.background_executor.advance_clock(Duration::from_millis(200));
        owner.update(cx, |owner, cx| {
            owner.session.reader_preferences.toc_width = 400.0;
            owner.schedule(cx);
        });
        cx.background_executor.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        worker.dispatch(|| ()).recv().await.unwrap();
        assert!(!path.exists(), "a later edit extends the quiet period");
        owner.update(cx, |owner, cx| {
            owner.session.navigation = saved_navigation();
            owner.save_now(cx);
        });
        worker.dispatch(|| ()).recv().await.unwrap();
        let immediate = UiSession::load(&path);
        assert_eq!(immediate.navigation, saved_navigation());
        assert_eq!(immediate.reader_preferences.toc_width, 400.0);
        owner.update(cx, |owner, _| owner.session.reader_preferences.toc_width = 500.0);
        cx.background_executor.advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        worker.dispatch(|| ()).recv().await.unwrap();
        let delayed = UiSession::load(&path);
        assert_eq!(delayed.navigation, immediate.navigation);
        assert_eq!(delayed.reader_preferences.toc_width, 500.0);
        cx.run_until_parked();
        assert!(!owner.read_with(cx, |owner, _| owner.worker_running));
    }

    #[test]
    fn failed_atomic_write_preserves_the_previous_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = session_path(directory.path());
        let original = UiSession { navigation: saved_navigation(), ..Default::default() };
        original.save(&path);
        let failed = atomic_session_write(&path, |file| {
            file.write_all(b"{truncated")?;
            Err(io::Error::other("simulated interrupted write"))
        });
        assert!(failed.is_err());
        assert_eq!(UiSession::load(&path).navigation, original.navigation);
    }
}
