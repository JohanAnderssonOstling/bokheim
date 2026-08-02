use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use bokheim_ui_runtime::BlockingWorker;
use browser_ui::{BrowserSession, BrowserWindowGeometry};
use gpui::App;
use reader_ui::{ReaderPreferences, ReaderWindowGeometry};
use serde::{Deserialize, Serialize};

const SESSION_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiSession {
    version: u32,
    pub browser: BrowserSession,
    pub browser_window: Option<BrowserWindowGeometry>,
    pub reader_preferences: ReaderPreferences,
    pub reader_window: Option<ReaderWindowGeometry>,
}

impl Default for UiSession {
    fn default() -> Self {
        Self { version: SESSION_VERSION, browser: BrowserSession::default(), browser_window: None, reader_preferences: ReaderPreferences::default(), reader_window: None }
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
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                log::error!("failed to create GPUI session directory: {error}");
                return;
            }
        }
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(error) = fs::write(path, bytes) {
                    log::error!("failed to save GPUI session: {error}");
                }
            }
            Err(error) => {
                log::error!("failed to serialize GPUI session: {error}");
            }
        }
    }
}

pub fn session_path(app_data: &Path) -> PathBuf {
    app_data.join("gpui-ui-session.json")
}

#[derive(Clone)]
pub struct SessionSaveScheduler {
    session: Rc<RefCell<UiSession>>,
    path: PathBuf,
    state: Rc<RefCell<SaveSchedule>>,
    blocking_worker: BlockingWorker,
}

#[derive(Default)]
struct SaveSchedule {
    deadline: Option<Instant>,
    worker_running: bool,
}

impl SessionSaveScheduler {
    pub fn new(session: Rc<RefCell<UiSession>>, path: PathBuf, blocking_worker: BlockingWorker) -> Self {
        Self { session, path, state: Rc::new(RefCell::new(SaveSchedule::default())), blocking_worker }
    }

    pub fn schedule(&self, cx: &mut App) {
        {
            let mut state = self.state.borrow_mut();
            state.deadline = Some(Instant::now() + Duration::from_millis(300));
            if state.worker_running {
                return;
            }
            state.worker_running = true;
        }

        let scheduler = self.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_| {
            loop {
                let wait = {
                    let state = scheduler.state.borrow();
                    let Some(deadline) = state.deadline else {
                        break;
                    };
                    deadline.saturating_duration_since(Instant::now())
                };
                executor.timer(wait).await;

                let should_save = {
                    let mut state = scheduler.state.borrow_mut();
                    match state.deadline {
                        Some(deadline) if deadline <= Instant::now() => {
                            state.deadline = None;
                            true
                        }
                        Some(_) => false,
                        None => {
                            state.worker_running = false;
                            break;
                        }
                    }
                };
                if should_save {
                    let session = scheduler.session.borrow().clone();
                    let path = scheduler.path.clone();
                    let save = scheduler.blocking_worker.dispatch(move || session.save(&path));
                    let _ = save.recv().await;
                    let mut state = scheduler.state.borrow_mut();
                    if state.deadline.is_none() {
                        state.worker_running = false;
                        break;
                    }
                }
            }
        })
        .detach();
    }

    pub fn flush(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.deadline = None;
            state.worker_running = false;
        }
        let session = self.session.borrow().clone();
        let path = self.path.clone();
        let save = self.blocking_worker.dispatch(move || session.save(&path));
        let _ = save.recv_blocking();
    }
}
