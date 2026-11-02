//! Session-owned progress and cancellation, removed when the operation exits.
use crate::BackendError;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
use uuid::Uuid;

pub(crate) trait JobCancellation {
    fn cancel(&self);
}
impl JobCancellation for Rc<Cell<bool>> {
    fn cancel(&self) {
        self.set(true);
    }
}
impl JobCancellation for library_annotation_export::ExportControl {
    fn cancel(&self) {
        self.cancel();
    }
}
struct Job<P, C> {
    progress: P,
    control: C,
}
pub(crate) struct JobRegistry<P, C>(Rc<RefCell<HashMap<Uuid, Job<P, C>>>>);
impl<P, C> Default for JobRegistry<P, C> {
    fn default() -> Self {
        Self(Rc::default())
    }
}
pub(crate) struct JobGuard<P, C> {
    jobs: Rc<RefCell<HashMap<Uuid, Job<P, C>>>>,
    id: Uuid,
    pub(crate) control: C,
}
impl<P, C: Clone + JobCancellation> JobRegistry<P, C> {
    pub(crate) fn prepare(&self, id: Uuid, progress: P, control: C, duplicate: &str) -> Result<JobGuard<P, C>, BackendError> {
        let mut jobs = self.0.borrow_mut();
        if jobs.contains_key(&id) {
            return Err(BackendError::message(duplicate));
        }
        jobs.insert(id, Job { progress, control: control.clone() });
        Ok(JobGuard { jobs: self.0.clone(), id, control })
    }
    pub(crate) fn cancel(&self, id: Uuid) -> bool {
        if let Some(job) = self.0.borrow().get(&id) {
            job.control.cancel();
            true
        } else {
            false
        }
    }
    pub(crate) fn progress(&self, id: Uuid) -> Option<P>
    where
        P: Clone,
    {
        self.0.borrow().get(&id).map(|job| job.progress.clone())
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}
impl<P, C> JobGuard<P, C> {
    pub(crate) fn set_progress(&self, progress: P) {
        if let Some(job) = self.jobs.borrow_mut().get_mut(&self.id) {
            job.progress = progress;
        }
    }
}
impl<P, C> Drop for JobGuard<P, C> {
    fn drop(&mut self) {
        self.jobs.borrow_mut().remove(&self.id);
    }
}
