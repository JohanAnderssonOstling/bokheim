//! Dedicated WASM entry point: deliberately does not initialize the backend.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use library_backend::library::web_reader::{connect, Reader};

#[wasm_bindgen(module = "/pdfium.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn initialize_pdfium() -> Result<(), JsValue>;
}

#[wasm_bindgen]
pub fn execute(bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    execute_with_source(bytes, JsValue::UNDEFINED)
}

fn execute_with_source(bytes: &[u8], source: JsValue) -> Result<Vec<u8>, JsValue> {
    let response = (|| -> Result<_, app::BackendError> {
        let request = rmp_serde::from_slice(bytes).map_err(app::BackendError::operation)?;
        match request {
            app::host::cpu::CpuRequest::Book { task, .. } => {
                let reader = Reader::new(source).map_err(app::BackendError::operation)?;
                app::host::cpu::execute_book_with_mobi(task, Box::new(reader), |_, _| Err("subject page inspection is not supported for this format".into()))
            }
            request => app::host::cpu::execute(request),
        }
    })();
    // Application failures travel as typed results inside the byte payload.
    // The outer transport failure is reserved for a broken worker/connection.
    rmp_serde::to_vec_named(&response).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(js_name = startCpuWorker)]
pub fn start() -> Result<(), JsValue> {
    let scope = client_platform_web::transport::scope::Scope::current();
    let worker = scope.clone();
    scope.receive(move |input| {
        let worker = worker.clone();
        wasm_bindgen_futures::spawn_local(async move {
            use client_platform_web::transport::{byte_reply, error_message, object, protocol::CpuRequest};
            let request = match CpuRequest::decode(&input) {
                Ok(request) => request,
                Err(error) => {
                    let _ = worker.post_message(&object(&[("transport", "endpoint_failed".into()), ("error", error.into())]));
                    return;
                }
            };
            let CpuRequest { id, bytes, source } = request;
            let response = async {
                if !source.is_undefined() {
                    connect(&source).await?;
                }
                if matches!(rmp_serde::from_slice::<app::host::cpu::CpuRequest>(&bytes),
                    Ok(app::host::cpu::CpuRequest::Book {
                        task: app::host::cpu::BookTask::Thumbnail { extension }, ..
                    }) if extension == "pdf")
                    || matches!(rmp_serde::from_slice::<app::host::cpu::CpuRequest>(&bytes), Ok(app::host::cpu::CpuRequest::Book { task: app::host::cpu::BookTask::Inspect { format: book_model::BookFormat::Pdf, .. }, .. }))
                {
                    initialize_pdfium().await?;
                }
                // Inspection checks subject codes against the taxonomy. This
                // module bundles none, so it loads the database worker's
                // snapshot, usually from the HTTP cache, on first use.
                if let Ok(app::host::cpu::CpuRequest::Book { taxonomy: Some(artifact), .. }) = rmp_serde::from_slice::<app::host::cpu::CpuRequest>(&bytes) {
                    if subject_projection::loaded_unified_taxonomy_revision_id().is_none() {
                        let snapshot = update_client::fetch::download(&artifact).await.map_err(|error| JsValue::from_str(&error))?;
                        subject_projection::installed::install_snapshot(&snapshot).map_err(|error| JsValue::from_str(&error))?;
                    }
                }
                execute_with_source(&bytes, source)
            }
            .await;
            let (result, transfer) = byte_reply(&[("id", id.into())], response.map_err(|error| error_message(&error)));
            let _ = worker.post_message_with_transfer(&result, &transfer);
        });
    });
    let ready = js_sys::Object::new();
    js_sys::Reflect::set(&ready, &"ready".into(), &true.into())?;
    scope.post_message(&ready)
}
