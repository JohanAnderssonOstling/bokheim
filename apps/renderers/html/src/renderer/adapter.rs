use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use floem::ViewId;
use floem::action::exec_after;
use floem::reactive::{RwSignal, SignalGet, SignalUpdate, create_rw_signal};

use super::boundary::{RendererCommand, RendererEvent, RendererHost};
use html_view_core::{BlockingJob, BlockingJobSpawner};

type EventCallback = Rc<dyn Fn(RendererEvent)>;

struct FloemBlockingJobSpawner;

impl BlockingJobSpawner for FloemBlockingJobSpawner {
    fn spawn(&self, job: BlockingJob) {
        #[cfg(not(target_family = "wasm"))]
        rayon::spawn(job);
        #[cfg(target_family = "wasm")]
        job();
    }
}

pub struct RendererHandle {
    id: ViewId,
    commands: RwSignal<Vec<RendererCommand>>,
}

impl Clone for RendererHandle {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for RendererHandle {}

impl RendererHandle {
    pub fn apply(&self, command: RendererCommand) {
        self.commands.update(|commands| commands.push(command));
        self.id.request_paint();
    }
}

pub(crate) struct FloemAdapter {
    id: ViewId,
    commands: RwSignal<Vec<RendererCommand>>,
    event_callback: RefCell<Option<EventCallback>>,
    clipboard: RefCell<Option<arboard::Clipboard>>,
}

impl FloemAdapter {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self { id: ViewId::new(), commands: create_rw_signal(Vec::new()), event_callback: RefCell::new(None), clipboard: RefCell::new(arboard::Clipboard::new().ok()) })
    }

    pub(crate) fn id(&self) -> ViewId {
        self.id
    }

    pub(crate) fn handle(&self) -> RendererHandle {
        RendererHandle { id: self.id, commands: self.commands }
    }

    pub(crate) fn set_event_callback(&self, callback: impl Fn(RendererEvent) + 'static) {
        *self.event_callback.borrow_mut() = Some(Rc::new(callback));
    }

    pub(crate) fn take_commands(&self) -> Vec<RendererCommand> {
        let commands = self.commands.get_untracked();
        if !commands.is_empty() {
            self.commands.set(Vec::new());
        }
        commands
    }
}

impl RendererHost for FloemAdapter {
    fn request_repaint(&self) {
        self.id.request_paint();
    }

    fn request_style(&self) {
        self.id.request_style();
    }

    fn schedule(&self, delay: Duration, f: Box<dyn FnOnce() + Send>) {
        exec_after(delay, move |_| f());
    }

    fn schedule_repaint(&self, delay: Duration) {
        let id = self.id;
        exec_after(delay, move |_| id.request_paint());
    }

    fn blocking_job_spawner(&self) -> std::sync::Arc<dyn BlockingJobSpawner> {
        std::sync::Arc::new(FloemBlockingJobSpawner)
    }

    fn resource_waker(&self) -> Option<std::sync::Arc<dyn Fn() + Send + Sync>> {
        None
    }

    fn set_clipboard(&self, text: &str) -> Result<(), String> {
        if self.clipboard.borrow().is_none() {
            *self.clipboard.borrow_mut() = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = self.clipboard.borrow_mut().as_mut() {
            clipboard.set_text(text.to_owned()).map_err(|error| format!("arboard set_text failed: {error}"))
        } else {
            floem::Clipboard::set_contents(text.to_owned()).map_err(|error| format!("floem clipboard failed: {error:?}"))
        }
    }

    fn set_clipboard_image(&self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        if self.clipboard.borrow().is_none() {
            *self.clipboard.borrow_mut() = arboard::Clipboard::new().ok();
        }
        self.clipboard
            .borrow_mut()
            .as_mut()
            .ok_or_else(|| "failed to initialize arboard clipboard".to_string())?
            .set_image(arboard::ImageData { width, height, bytes: Cow::Owned(rgba) })
            .map_err(|error| format!("arboard set_image failed: {error}"))
    }

    fn emit(&self, event: RendererEvent) {
        if let Some(callback) = self.event_callback.borrow().as_ref() {
            callback(event);
        }
    }
}
