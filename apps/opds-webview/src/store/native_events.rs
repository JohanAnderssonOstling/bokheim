//! Give GTK a bounded share of the GPUI thread, even with continuously ready sources.
use gtk::glib::MainContext;
use std::time::{Duration, Instant};

pub(super) fn pump(context: &MainContext) {
    let deadline = Instant::now() + Duration::from_millis(2);
    for _ in 0..64 {
        if Instant::now() >= deadline || !context.pending() {
            break;
        }
        context.iteration(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib::{ControlFlow, Priority, idle_source_new};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn yields_with_a_continuously_ready_source() {
        let context = MainContext::new();
        let dispatched = Arc::new(AtomicUsize::new(0));
        let count = dispatched.clone();
        let source = idle_source_new(None, Priority::DEFAULT_IDLE, move || {
            count.fetch_add(1, Ordering::Relaxed);
            ControlFlow::Continue
        });
        source.attach(Some(&context));
        pump(&context);
        source.destroy();
        assert!((1..=64).contains(&dispatched.load(Ordering::Relaxed)));
    }
}
