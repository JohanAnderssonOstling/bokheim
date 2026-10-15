//! Application services wired to the platform's elected browser host.
use super::Coordinator;
use client_platform_web::transport::host::{HostedApplication, SharedWorkerHost, WorkerHost};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::MessagePort;

struct Application {
    options: super::WorkerOptions,
    coordinator: RefCell<Option<Coordinator>>,
}
impl HostedApplication for Application {
    fn start(self: Rc<Self>, host: WorkerHost) -> Result<(), JsValue> {
        let coordinator = Coordinator::with_host(host, self.options)?;
        *self.coordinator.borrow_mut() = Some(coordinator);
        Ok(())
    }
    fn stop(&self) {
        if let Some(coordinator) = self.coordinator.borrow_mut().take() {
            coordinator.suspend();
        }
    }
    fn receive(&self, _: JsValue) -> bool {
        false
    }
    fn connected(&self, _: &MessagePort) {}
}

#[derive(Clone)]
pub struct SharedCoordinator(SharedWorkerHost);
impl SharedCoordinator {
    pub fn new() -> Self {
        Self::with_options(Default::default())
    }
    pub fn with_options(options: super::WorkerOptions) -> Self {
        let application = Rc::new(Application { options, coordinator: RefCell::new(None) });
        Self(SharedWorkerHost::new(application, "bokheim-tab-"))
    }
    pub fn fail(&self, error: &str) {
        self.0.fail(error);
    }
    pub fn connect(&self, port: MessagePort) {
        self.0.connect(port);
    }
}
