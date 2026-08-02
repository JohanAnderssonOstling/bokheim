use std::rc::Rc;
use std::thread::JoinHandle;

use browser_ui::ResolveLibrary;
use epub_svelte_lib::APP_NAME_LOWER;
use epub_svelte_lib::app_core::AppCore;
use epub_svelte_lib::home::BackendContext;
use epub_svelte_lib::scanner::LibraryEvent;
use gpui::Global;

pub struct LibraryRegistry {
    core: AppCore,
    events: async_channel::Receiver<(String, LibraryEvent)>,
    _event_thread: JoinHandle<()>,
}

impl Global for LibraryRegistry {}

impl LibraryRegistry {
    pub fn open(context: &BackendContext) -> Self {
        let event_thread_name = format!("{APP_NAME_LOWER}-app-events");
        let core = AppCore::new(context.clone()).expect("failed to initialize shared application backend");
        let backend_events = core.take_events().expect("desktop application owns the backend event receiver");
        let (event_sender, events) = async_channel::unbounded();
        let event_thread = std::thread::Builder::new()
            .name(event_thread_name)
            .spawn(move || {
                while let Ok(app_event) = backend_events.recv() {
                    if event_sender.send_blocking((app_event.library_id.as_str().to_owned(), app_event.event)).is_err() {
                        break;
                    }
                }
            })
            .expect("failed to start application event bridge");

        Self { core, events, _event_thread: event_thread }
    }

    pub fn resolver(&self) -> ResolveLibrary {
        let core = self.core.clone();
        Rc::new(move |library_id| {
            core.library_handle(library_id)
                .map_err(|error| {
                    log::error!("failed to open library {library_id}: {error}");
                    error
                })
                .ok()
        })
    }

    pub fn events(&self) -> async_channel::Receiver<(String, LibraryEvent)> {
        self.events.clone()
    }

    pub fn core(&self) -> AppCore {
        self.core.clone()
    }
}
