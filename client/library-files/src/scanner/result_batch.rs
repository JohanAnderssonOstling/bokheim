//! Combine nearby worker completions without waiting for an entire scan.
use crossbeam_channel::Receiver;
use std::time::{Duration, Instant};

const COLLECTION_WINDOW: Duration = Duration::from_millis(25);

pub(super) fn collect<T>(receiver: &Receiver<T>, first: T, limit: usize) -> Vec<T> {
    collect_for(receiver, first, limit, COLLECTION_WINDOW)
}

fn collect_for<T>(receiver: &Receiver<T>, first: T, limit: usize, window: Duration) -> Vec<T> {
    let deadline = Instant::now() + window;
    let mut batch = Vec::with_capacity(limit.max(1));
    batch.push(first);
    while batch.len() < limit {
        // Already-ready work never waits, including a zero-window benchmark.
        match receiver.try_recv() {
            Ok(item) => batch.push(item),
            Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            Err(crossbeam_channel::TryRecvError::Empty) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() { break; }
                match receiver.recv_timeout(remaining) {
                    Ok(item) => batch.push(item),
                    Err(_) => break,
                }
            }
        }
    }
    batch
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_batches_and_the_final_result_do_not_wait() {
        let (tx, rx) = crossbeam_channel::unbounded();
        for i in 1..5 { tx.send(i).unwrap(); }
        assert_eq!(collect(&rx, 0, 3), vec![0, 1, 2]);
        assert_eq!(rx.len(), 2);
        assert_eq!(collect(&rx, 9, 1), vec![9]);
        assert_eq!(rx.len(), 2, "final result must not consume another batch");
    }
    #[test]
    fn partial_batch_survives_producer_disconnect() {
        let (tx, rx) = crossbeam_channel::unbounded();
        tx.send(1).unwrap();
        drop(tx);
        assert_eq!(collect(&rx, 0, 64), vec![0, 1]);
    }
    #[test]
    fn sparse_producers_are_bounded_by_a_deadline() {
        let (_tx, rx) = crossbeam_channel::unbounded::<u32>();
        let start = Instant::now();
        assert_eq!(collect(&rx, 0, 64), vec![0]);
        assert!(start.elapsed() < Duration::from_secs(1), "must not wait for a missing worker indefinitely");
    }

    /// Controlled worker arrivals, not a filesystem or end-to-end sync benchmark.
    #[test]
    #[ignore]
    fn benchmark_staggered_worker_results() {
        for delay in [Duration::ZERO, COLLECTION_WINDOW] {
            for sample in 0..5 {
                let (tx, rx) = crossbeam_channel::unbounded();
                let producer = std::thread::spawn(move || {
                    for i in 0..64 {
                        tx.send(i).unwrap();
                        std::thread::sleep(Duration::from_millis(2));
                    }
                });
                let start = Instant::now();
                let mut remaining = 64;
                let mut batches = 0;
                let mut seen = Vec::new();
                while remaining > 0 {
                    let first = rx.recv().unwrap();
                    let batch = collect_for(&rx, first, remaining.min(64), delay);
                    remaining -= batch.len();
                    batches += 1;
                    seen.extend(batch);
                }
                let elapsed = start.elapsed();
                producer.join().unwrap();
                assert_eq!(seen, (0..64).collect::<Vec<_>>());
                println!("worker_batch_profile window_ms={} sample={} results=64 batches={} elapsed_ms={:.3}", delay.as_millis(), sample, batches, elapsed.as_secs_f64()*1000.0);
            }
        }
    }
}
