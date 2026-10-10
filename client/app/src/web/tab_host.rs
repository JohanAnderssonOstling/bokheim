//! Application worker roles over browser transport.

pub use client_platform_web::transport::tab::WebConfiguration;
use client_platform_web::transport::tab::{TabConnection, WorkerSpec};
use web_sys::MessagePort;

/// Keeps the browser transport alive.
pub struct WebConnection {
    pub(crate) worker: web_sys::SharedWorker,
    transport: TabConnection,
}
impl WebConnection {
    pub async fn connect(config: WebConfiguration) -> Result<Self, String> {
        let transport = TabConnection::connect(
            &config.broker_url,
            &config.broker_name,
            &config.lifetime_lock,
            move |role, id| {
                let url = config.workers.get(role).ok_or_else(|| js_sys::Error::new("Unknown worker role"))?.clone();
                Ok(WorkerSpec { url, name: format!("{}-{id}", config.worker_name_prefix) })
            },
            |_| {},
            client_platform_web::transport::tab::reload_page,
        )
        .await?;
        Ok(Self { worker: transport.worker().clone(), transport })
    }
    pub fn port(&self) -> MessagePort {
        self.transport.port()
    }
}
