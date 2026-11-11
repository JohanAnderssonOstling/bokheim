//! Synchronous book parsers and PDFium, isolated from the UI and storage owner.
#![cfg(target_arch = "wasm32")]

mod import_hasher;

use operation_failure::OperationError;
use app::BackendError;
use app::host::cpu::{self, BookTask, CpuRequest};
use book_model::BookFormat;
use library_backend::library::web_reader::{Reader, connect};
use wasm_bindgen::prelude::*;
use wasm_callback::future_to_promise_aborting;

#[wasm_bindgen(module = "/pdfium.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn initialize_pdfium() -> Result<(), JsValue>;
}

#[wasm_bindgen]
pub fn execute(bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    execute_request(rmp_serde::from_slice(bytes).map_err(BackendError::operation), JsValue::UNDEFINED)
}

fn execute_request(request: Result<CpuRequest, BackendError>, source: JsValue) -> Result<Vec<u8>, JsValue> {
    let response = request.and_then(|request| match request {
        CpuRequest::Book { task, .. } => {
            let reader = Reader::new(source).map_err(BackendError::operation)?;
            cpu::execute_book_with_mobi(task, Box::new(reader), |_, _| Err("subject page inspection is not supported for this format".into()))
        }
        request => cpu::execute(request),
    });
    // Application failures travel as typed results inside the byte payload.
    // The outer transport failure is reserved for a broken worker/connection.
    rmp_serde::to_vec_named(&response).map_err(|error| OperationError::operation(error).into_js())
}

#[wasm_bindgen(js_name = startCpuWorker)]
pub fn start() -> js_sys::Promise {
    future_to_promise_aborting(async {
        initialize().await?;
        Ok(JsValue::UNDEFINED)
    })
}

async fn initialize() -> Result<(), JsValue> {
    client_platform_runtime::executor::initialize_storage_executor().map_err(|error| OperationError::operation(error).into_js())?;
    client_platform_web::sqlite::initialize_reference_storage().await.map_err(|error| OperationError::operation(error).into_js())?;
    app::host::cpu::initialize_bisac_reference().await.map_err(|error| OperationError::operation(error).into_js())?;
    let scope = client_platform_web::transport::scope::Scope::current();
    let worker = scope.clone();
    scope.receive(move |input| {
        let worker = worker.clone();
        wasm_bindgen_futures::spawn_local(async move {
            use client_platform_web::transport::{byte_reply, object, protocol::CpuRequest as TransportRequest};
            let request = match TransportRequest::decode(&input) {
                Ok(request) => request,
                Err(error) => {
                    let _ = worker.post_message(&object(&[("transport", "endpoint_failed".into()), ("error", error.into())]));
                    return;
                }
            };
            let TransportRequest { id, bytes, source } = request;
            let response = async {
                let request = rmp_serde::from_slice(&bytes).map_err(BackendError::operation);
                if !source.is_undefined() {
                    connect(&source).await?;
                }
                if matches!(&request,
                    Ok(CpuRequest::Book {
                        task: BookTask::Thumbnail { extension }, ..
                    }) if extension == "pdf")
                    || matches!(&request, Ok(CpuRequest::Book { task: BookTask::Inspect { format: BookFormat::Pdf, .. } | BookTask::TextMatches { format: BookFormat::Pdf, .. } | BookTask::IndexText { format: BookFormat::Pdf }, .. }))
                {
                    initialize_pdfium().await?;
                }
                // Inspection checks subject codes against the taxonomy. This
                // worker loads the same app asset or published snapshot as the
                // database worker, usually from the HTTP cache, on first use.
                if let Ok(CpuRequest::Book { task: BookTask::Inspect { .. }, taxonomy }) = &request {
                    if subject_projection::loaded_unified_taxonomy_revision_id().is_none() {
                        match taxonomy {
                            Some(artifact) => {
                                let snapshot = update_client::fetch::download(artifact).await.map_err(|error| OperationError::operation(error).into_js())?;
                                subject_projection::installed::install_published_snapshot(&snapshot).map_err(|error| OperationError::operation(error).into_js())?;
                            }
                            None => app::host::cpu::initialize_bundled_taxonomy().await.map_err(|error| OperationError::operation(error).into_js())?,
                        }
                    }
                }
                execute_request(request, source)
            }
            .await;
            let (result, transfer) = byte_reply(&[("id", id.into())], response.map_err(OperationError::from));
            let _ = worker.post_message_with_transfer(&result, &transfer);
        });
    });
    let ready = js_sys::Object::new();
    js_sys::Reflect::set(&ready, &"ready".into(), &true.into())?;
    scope.post_message(&ready)
}
