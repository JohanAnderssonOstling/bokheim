//! Bounded requests for the synchronous browser parser.
use std::collections::VecDeque;

pub struct CpuJobs<T> {
    pending: VecDeque<(u32, T)>,
    active: Option<(u32, T)>,
    limit: usize,
    ready: bool,
}
impl<T> CpuJobs<T> {
    pub fn new(limit: usize) -> Self {
        Self { pending: VecDeque::new(), active: None, limit, ready: false }
    }
    pub fn request(&mut self, id: u32, job: T) -> Result<(), (String, T)> {
        if self.pending.len() + usize::from(self.active.is_some()) >= self.limit {
            return Err(("CPU queue is full; retry the job".into(), job));
        }
        self.pending.push_back((id, job));
        Ok(())
    }
    /// Queued work can be removed. Active work retains its slot until completion.
    pub fn cancel_pending(&mut self, id: u32) -> Option<T> {
        let index = self.pending.iter().position(|(pending, _)| *pending == id)?;
        self.pending.remove(index).map(|(_, job)| job)
    }
    pub fn ready(&mut self) {
        self.ready = true;
    }
    pub fn next(&mut self) -> Option<(u32, &mut T)> {
        if !self.ready || self.active.is_some() {
            return None;
        }
        self.active = self.pending.pop_front();
        self.active.as_mut().map(|(id, job)| (*id, job))
    }
    pub fn complete(&mut self, id: u32) -> Result<T, String> {
        match self.active.take() {
            Some((active, job)) if active == id => Ok(job),
            other => {
                self.active = other;
                Err("Unexpected CPU worker response".into())
            }
        }
    }
    pub fn fail(&mut self) -> Vec<T> {
        self.ready = false;
        self.active.take().into_iter().chain(self.pending.drain(..)).map(|(_, job)| job).collect()
    }
}
