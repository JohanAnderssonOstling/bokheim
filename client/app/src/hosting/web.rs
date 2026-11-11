//! One page and a shared-memory event-loop owner, using the UI's Wasm bundle.
use crate::{
    app::{AppBackend, AppDataLocation},
    AppClient, AppStartupState, BackendError,
};
use client_platform_web::{
    import_worker,
    transport::{cpu_client, error_message, import_io},
    web_storage::{self, StorageInitError},
};
use wasm_bindgen::prelude::*;
use client_protocol::OperationError;

#[wasm_bindgen(module = "/src/hosting/single_tab.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = acquireTab)]
    async fn acquire_tab() -> Result<(), JsValue>;
}

impl AppClient {
    pub async fn connect_web(asset_base: &str) -> Result<(Self, AppStartupState), BackendError> {
        acquire_tab().await.map_err(|error| BackendError::classified(crate::Failure::StorageBusy, error_message(&error)))?;
        let base = web_sys::window().ok_or("Browser startup requires a window")?.location().href().map_err(|error| BackendError::operation(OperationError::from(error)))?;
        let base = web_sys::Url::new_with_base(&format!("{}/", asset_base.trim_end_matches('/')), &base).map_err(|error| BackendError::operation(OperationError::from(error)))?.href();
        let shim = format!("{base}web_gpui.js");
        let (reply, ready) = tokio::sync::oneshot::channel();
        wasm_thread::Builder::new()
            .name("bokheim-storage".into())
            .wasm_bindgen_shim_url(shim)
            .spawn_async(move || async move {
                let permit = crate::executor::defer_background_work();
                let (stopped, shutdown) = tokio::sync::oneshot::channel();
                let outcome = async {
                    js_sys::Reflect::set(&js_sys::global(), &"__BOKHEIM_ASSET_BASE__".into(), &JsValue::from_str(&base)).map_err(|error| BackendError::operation(OperationError::from(error)))?;
                    cpu_client::initialize(format!("{base}cpu_worker_loader.js"));
                    import_worker::initialize(&format!("{base}import_worker_loader.js")).map_err(BackendError::operation)?;
                    import_io::initialize().map_err(BackendError::operation)?;
                    crate::executor::initialize_storage_executor().map_err(BackendError::operation)?;
                    web_storage::initialize().await.map_err(|error| match error {
                        StorageInitError::Busy(error) => BackendError::classified(crate::Failure::StorageBusy, error),
                        StorageInitError::Failed(error) => BackendError::operation(error),
                    })?;
                    crate::web::initialize_reference_data().await?;
                    let context = crate::app::BackendContext::initialize(AppDataLocation::PlatformLocal("bokheim".to_owned())).await?;
                    let backend = AppBackend::new(context).await?;
                    let startup = backend.startup_state().await?;
                    Ok((super::client(backend, Some(stopped)), startup))
                }
                .await;
                let succeeded = outcome.is_ok();
                if reply.send(outcome).is_ok() && succeeded {
                    permit.release();
                    let _ = shutdown.await;
                }
            })
            .map_err(BackendError::operation)?;
        crate::executor::timeout(std::time::Duration::from_secs(120), ready)
            .await
            .map_err(|_| BackendError::classified(crate::Failure::TimedOut, "The storage worker did not finish starting"))?
            .map_err(|_| BackendError::message("The storage worker stopped during startup"))?
    }
}
