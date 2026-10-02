//! Library-owned thumbnail-work coordination.

pub(crate) use std::sync::Mutex;

pub(crate) use library_model::ScanProgress;

#[derive(Default)]
pub struct ThumbnailWork {
    pub(crate) state: Mutex<ThumbnailWorkState>,
}

#[derive(Default)]
pub(crate) struct ThumbnailWorkState {
    pub(crate) batch_active: bool,
    pub(crate) generations_running: usize,
    pub(crate) progress: ScanProgress,
}

impl ThumbnailWork {
    pub fn running(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.batch_active || state.generations_running > 0
    }
    pub fn progress(&self) -> Option<ScanProgress> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        (state.batch_active || state.generations_running > 0).then_some(state.progress)
    }
    pub fn waiting(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        (state.batch_active || state.generations_running > 0) && state.generations_running == 0
    }
}
