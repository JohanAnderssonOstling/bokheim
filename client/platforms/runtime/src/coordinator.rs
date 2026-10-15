//! Bounded queues own their payloads; browser adapters supply ports and replies.
use std::collections::{HashMap, VecDeque};

struct LibraryJobs<T> {
    active: Vec<T>,
    pending: Vec<T>,
}
pub struct SyncJobs<T> {
    libraries: HashMap<String, LibraryJobs<T>>,
    count: usize,
    limit: usize,
    failure: Option<String>,
}
impl<T> SyncJobs<T> {
    pub fn new(limit: usize) -> Self {
        Self { libraries: HashMap::new(), count: 0, limit, failure: None }
    }
    pub fn request(&mut self, key: String, request: T) -> Result<bool, (String, T)> {
        if let Some(error) = &self.failure {
            return Err((error.clone(), request));
        }
        if self.count >= self.limit {
            return Err(("Sync queue is full".into(), request));
        }
        self.count += 1;
        let jobs = self.libraries.entry(key).or_insert_with(|| LibraryJobs { active: Vec::new(), pending: Vec::new() });
        if jobs.active.is_empty() {
            jobs.active.push(request);
            Ok(true)
        } else {
            jobs.pending.push(request);
            Ok(false)
        }
    }
    pub fn active_mut(&mut self, key: &str) -> Option<&mut T> {
        self.libraries.get_mut(key)?.active.last_mut()
    }
    pub fn complete(&mut self, key: &str) -> Vec<T> {
        let Some(mut jobs) = self.libraries.remove(key) else {
            return Vec::new();
        };
        self.count -= jobs.active.len();
        let completed = jobs.active;
        if !jobs.pending.is_empty() {
            jobs.active = jobs.pending;
            jobs.pending = Vec::new();
            self.libraries.insert(key.to_owned(), jobs);
        }
        completed
    }
    pub fn fail(&mut self, error: String) -> Vec<T> {
        self.failure = Some(error);
        self.count = 0;
        self.libraries.drain().flat_map(|(_, jobs)| jobs.active.into_iter().chain(jobs.pending)).collect()
    }
    pub fn count(&self) -> usize {
        self.count
    }
}

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
        if self.active.as_ref().map(|(active, _)| *active) != Some(id) {
            return Err("Unexpected CPU worker response".into());
        }
        Ok(self.active.take().unwrap().1)
    }
    pub fn fail(&mut self) -> Vec<T> {
        self.ready = false;
        self.active.take().into_iter().chain(self.pending.drain(..)).map(|(_, job)| job).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_requests_coalesce_and_promote_the_latest_pending_input() {
        let mut jobs = SyncJobs::new(4);
        assert_eq!(jobs.request("a".into(), 1), Ok(true));
        assert_eq!(jobs.request("a".into(), 2), Ok(false));
        assert_eq!(jobs.request("a".into(), 3), Ok(false));
        assert_eq!(jobs.request("b".into(), 4), Ok(true));
        assert_eq!(jobs.request("a".into(), 5), Err(("Sync queue is full".into(), 5)));
        assert_eq!(jobs.complete("a"), vec![1]);
        assert_eq!(jobs.active_mut("a"), Some(&mut 3));
        assert_eq!(jobs.count(), 3);
        assert_eq!(jobs.complete("a"), vec![2, 3]);
        assert_eq!(jobs.complete("b"), vec![4]);
        assert_eq!(jobs.count(), 0);
        assert_eq!(jobs.request("a".into(), 6), Ok(true));
    }

    #[test]
    fn sync_failure_drains_active_and_queued_requests_and_is_terminal() {
        let mut jobs = SyncJobs::new(3);
        jobs.request("a".into(), 1).unwrap();
        jobs.request("a".into(), 2).unwrap();
        jobs.request("b".into(), 3).unwrap();
        let mut failed = jobs.fail("host stopped".into());
        failed.sort();
        assert_eq!(failed, vec![1, 2, 3]);
        assert_eq!(jobs.count(), 0);
        assert!(jobs.complete("a").is_empty());
        assert_eq!(jobs.request("a".into(), 4), Err(("host stopped".into(), 4)));
    }
}
