//! Opt-in operation timings for native library-scan profiling.
//! Reader/writer timings are nested inside the operation, not additional work.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

static EPOCH: OnceLock<Instant> = OnceLock::new();

#[derive(Default)]
pub struct IoTimings {
    read_ns: AtomicU64,
    write_ns: AtomicU64,
    seek_ns: AtomicU64,
    read_bytes: AtomicU64,
    write_bytes: AtomicU64,
}

pub struct ProfileSpan {
    stage: &'static str,
    detail: Option<(Instant, String, Arc<IoTimings>)>,
}

impl ProfileSpan {
    pub fn new(stage: &'static str, label: &str) -> Self {
        let detail = (!cfg!(target_arch = "wasm32") && log::log_enabled!(target: "library_scan_profile", log::Level::Debug)).then(|| {
            EPOCH.get_or_init(Instant::now);
            (Instant::now(), label.to_owned(), Arc::new(IoTimings::default()))
        });
        Self { stage, detail }
    }

    pub fn io(&self) -> Option<Arc<IoTimings>> {
        self.detail.as_ref().map(|(_, _, io)| io.clone())
    }
}

impl Drop for ProfileSpan {
    fn drop(&mut self) {
        if let Some((started, label, io)) = &self.detail {
            log::debug!(target: "library_scan_profile", "scan_detail stage={} start_ns={} elapsed_ns={} read_ns={} write_ns={} seek_ns={} read_bytes={} write_bytes={} thread={:?} label={label:?}",
                self.stage, started.duration_since(*EPOCH.get().unwrap()).as_nanos(), started.elapsed().as_nanos(),
                io.read_ns.load(Ordering::Relaxed), io.write_ns.load(Ordering::Relaxed), io.seek_ns.load(Ordering::Relaxed),
                io.read_bytes.load(Ordering::Relaxed), io.write_bytes.load(Ordering::Relaxed), std::thread::current().id());
        }
    }
}

pub struct ProfileIo<R> {
    inner: R,
    timings: Option<Arc<IoTimings>>,
}

impl<R> ProfileIo<R> {
    pub fn new(inner: R, timings: Option<Arc<IoTimings>>) -> Self {
        Self { inner, timings }
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for ProfileIo<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let Some(timings) = &self.timings else { return self.inner.read(buffer) };
        let started = Instant::now();
        let result = self.inner.read(buffer);
        timings.read_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if let Ok(length) = result {
            timings.read_bytes.fetch_add(length as u64, Ordering::Relaxed);
        }
        result
    }

    fn read_to_end(&mut self, buffer: &mut Vec<u8>) -> io::Result<usize> {
        let Some(timings) = &self.timings else { return self.inner.read_to_end(buffer) };
        let started = Instant::now();
        let before = buffer.len();
        let result = self.inner.read_to_end(buffer);
        timings.read_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        timings.read_bytes.fetch_add((buffer.len() - before) as u64, Ordering::Relaxed);
        result
    }
}

impl<R: Write> Write for ProfileIo<R> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(timings) = &self.timings else { return self.inner.write(buffer) };
        let started = Instant::now();
        let result = self.inner.write(buffer);
        timings.write_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if let Ok(length) = result {
            timings.write_bytes.fetch_add(length as u64, Ordering::Relaxed);
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<R: Seek> Seek for ProfileIo<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let Some(timings) = &self.timings else { return self.inner.seek(position) };
        let started = Instant::now();
        let result = self.inner.seek(position);
        timings.seek_ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        result
    }
}
