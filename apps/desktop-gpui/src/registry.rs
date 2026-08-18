use std::thread::JoinHandle;

use epub_svelte_lib::app::AppBackend;
use epub_svelte_lib::app::BackendContext;
use epub_svelte_lib::platform::scanner::LibraryEvent;
use epub_svelte_lib::{APP_NAME_LOWER, LibraryId};
use gpui::Global;

pub struct LibraryRegistry {
    core: AppBackend,
    events: async_channel::Receiver<(LibraryId, LibraryEvent)>,
    _event_thread: JoinHandle<()>,
}

impl Global for LibraryRegistry {}

impl LibraryRegistry {
    pub fn open(context: &BackendContext) -> Self {
        let event_thread_name = format!("{APP_NAME_LOWER}-app-events");
        let core = AppBackend::new_deferred_scanning(context.clone()).expect("failed to initialize shared application backend");
        let backend_events = core.take_events().expect("desktop application owns the backend event receiver");
        let (event_sender, events) = async_channel::unbounded();
        let event_thread = std::thread::Builder::new()
            .name(event_thread_name)
            .spawn(move || {
                while let Ok(app_event) = backend_events.recv() {
                    if event_sender.send_blocking((app_event.library_id, app_event.event)).is_err() {
                        break;
                    }
                }
            })
            .expect("failed to start application event bridge");

        Self { core, events, _event_thread: event_thread }
    }

    pub fn events(&self) -> async_channel::Receiver<(LibraryId, LibraryEvent)> {
        self.events.clone()
    }

    pub fn core(&self) -> AppBackend {
        self.core.clone()
    }
}
