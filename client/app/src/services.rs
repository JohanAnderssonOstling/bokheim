//! Scheduling retained for application-coordinated services.
use crate::executor::BackendExecutor;

pub(crate) struct Services {
    pub executor: BackendExecutor,
}
impl Services {
    pub fn new(executor: BackendExecutor) -> Self {
        Self { executor }
    }
}
