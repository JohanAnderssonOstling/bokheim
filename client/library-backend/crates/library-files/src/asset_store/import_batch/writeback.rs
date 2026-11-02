//! Start Linux writeback while the importer copies and inspects other books.
//! These are optional hints; the batch's final filesystem sync provides durability.
use std::fs::File;
use std::io::{self, Error, ErrorKind};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};

const MINIMUM_BYTES: u64 = 128 * 1024;
const QUEUE_CAPACITY: usize = 8;

#[derive(Default)]
pub(super) enum Writeback {
    #[default]
    Idle,
    Disabled,
    Running {
        jobs: SyncSender<File>,
        worker: JoinHandle<io::Result<()>>,
    },
}

impl Writeback {
    pub(super) fn queue(&mut self, file: &File, size: u64) {
        if size < MINIMUM_BYTES || matches!(self, Self::Disabled) {
            return;
        }
        // Keep the exact inode alive through staging renames or cancellation.
        let Ok(file) = file.try_clone() else {
            return;
        };
        if matches!(self, Self::Idle) {
            *self = Self::start(QUEUE_CAPACITY, client_platform_native::filesystem::start_file_writeback).unwrap_or(Self::Disabled);
        }
        self.enqueue(file);
    }

    fn start(capacity: usize, mut writeback: impl FnMut(&File) -> io::Result<()> + Send + 'static) -> io::Result<Self> {
        let (jobs, files) = mpsc::sync_channel::<File>(capacity);
        let worker = thread::Builder::new().name("book-import-writeback".into()).spawn(move || {
            for file in files {
                match writeback(&file) {
                    // Unsupported filesystems retain the ordinary final sync.
                    Err(error) if error.kind() == ErrorKind::Unsupported => break,
                    result => result?,
                }
            }
            Ok(())
        })?;
        Ok(Self::Running { jobs, worker })
    }

    fn enqueue(&self, file: File) {
        // Never delay copying for a hint, or accumulate unbounded descriptors.
        // A full queue or stopped worker is covered by the final filesystem sync.
        if let Self::Running { jobs, .. } = self {
            let _ = jobs.try_send(file);
        }
    }

    pub(super) fn finish(self) -> io::Result<()> {
        let Self::Running { jobs, worker } = self else { return Ok(()) };
        drop(jobs);
        worker.join().map_err(|_| Error::new(ErrorKind::Other, "import writeback worker stopped unexpectedly"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::fs::FileExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn drains_exact_file_after_rename_and_writer_close() {
        let root = tempfile::tempdir().unwrap();
        let mut original = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        original.write_all(b"copied book").unwrap();
        original.seek(SeekFrom::Start(3)).unwrap();
        let (release, ready) = mpsc::channel();
        let worker = Writeback::start(8, move |file| {
            ready.recv().unwrap();
            let mut bytes = [0; 11];
            file.read_exact_at(&mut bytes, 0)?;
            assert_eq!(&bytes, b"copied book");
            assert_eq!(file.try_clone()?.stream_position()?, 3);
            Ok(())
        })
        .unwrap();
        worker.enqueue(original.as_file().try_clone().unwrap());
        original.persist(root.path().join("published.epub")).unwrap();
        release.send(()).unwrap();
        worker.finish().unwrap();
    }

    #[test]
    fn full_queue_skips_hints_without_blocking_copying() {
        let (entered, active) = mpsc::channel();
        let (release, ready) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let worker = Writeback::start(1, move |_| {
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                entered.send(()).unwrap();
                ready.recv().unwrap();
            }
            Ok(())
        })
        .unwrap();
        let file = tempfile::tempfile().unwrap();
        worker.enqueue(file.try_clone().unwrap());
        active.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.enqueue(file.try_clone().unwrap());
        worker.enqueue(file);
        release.send(()).unwrap();
        worker.finish().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn writeback_errors_are_returned_and_unsupported_is_ignored() {
        for kind in [ErrorKind::Unsupported, ErrorKind::StorageFull, ErrorKind::Other] {
            let worker = Writeback::start(8, move |_| Err(kind.into())).unwrap();
            worker.enqueue(tempfile::tempfile().unwrap());
            let result = worker.finish();
            if kind == ErrorKind::Unsupported {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().kind(), kind);
            }
        }
    }

    #[test]
    fn worker_panic_is_reported() {
        let worker = Writeback::start(8, |_| panic!("injected worker failure")).unwrap();
        worker.enqueue(tempfile::tempfile().unwrap());
        assert!(worker.finish().is_err());
    }

    #[test]
    fn abandoning_batch_lets_worker_release_resources() {
        struct Finished(mpsc::Sender<()>);
        impl Drop for Finished {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (finished, done) = mpsc::channel();
        let guard = Finished(finished);
        let worker = Writeback::start(8, move |_| {
            let _ = &guard;
            Ok(())
        })
        .unwrap();
        worker.enqueue(tempfile::tempfile().unwrap());
        drop(worker);
        done.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn small_files_do_not_start_worker() {
        let mut writeback = Writeback::default();
        writeback.queue(&tempfile::tempfile().unwrap(), MINIMUM_BYTES - 1);
        assert!(matches!(writeback, Writeback::Idle));
    }
}
