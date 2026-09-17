//! Dedicated WASM entry point: deliberately does not initialize the backend.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

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
    let request = rmp_serde::from_slice(bytes).map_err(|error| JsValue::from_str(&error.to_string()))?;
    let response = match request {
        bokheim_backend::host::cpu::CpuRequest::Book { task } => {
            let reader = bokheim_backend::host::web::book_source::Reader::new(source).map_err(|e| JsValue::from_str(&e))?;
            bokheim_backend::host::cpu::execute_book(task, Box::new(reader))
        }
        request => bokheim_backend::host::cpu::execute(request),
    }
    .map_err(|error| JsValue::from_str(&error))?;
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
                    bokheim_backend::host::web::book_source::connect(&source).await?;
                }
                if matches!(rmp_serde::from_slice::<bokheim_backend::host::cpu::CpuRequest>(&bytes),
                    Ok(bokheim_backend::host::cpu::CpuRequest::Book {
                        task: bokheim_backend::host::cpu::BookTask::Thumbnail { extension }
                    }) if extension == "pdf") || matches!(rmp_serde::from_slice::<bokheim_backend::host::cpu::CpuRequest>(&bytes),
                    Ok(bokheim_backend::host::cpu::CpuRequest::Book {
                        task: bokheim_backend::host::cpu::BookTask::Inspect { format: book_model::BookFormat::Pdf, .. }
                    }))
                {
                    initialize_pdfium().await?;
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

/// Test fixture digest for the import contract. Production hashing stays in Rust.
#[cfg(feature = "web-runtime-tests")]
#[wasm_bindgen]
pub struct ImportHasher(blake3::Hasher);
#[cfg(feature = "web-runtime-tests")]
#[wasm_bindgen]
impl ImportHasher {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    pub fn finish(self) -> String {
        self.0.finalize().to_hex().to_string()
    }
}
#[cfg(feature = "web-runtime-tests")]
#[wasm_bindgen]
pub fn recover_import(manifest: &[u8], journal: &[u8]) -> Result<JsValue, JsValue> {
    bokheim_backend::host::web::import_worker::recover(manifest, journal)
}

#[wasm_bindgen]
pub fn start_import_worker(open: js_sys::Function, discover: js_sys::Function) -> Result<(), JsValue> {
    bokheim_backend::host::web::import_worker::start(open, discover)
}

#[cfg(feature = "web-runtime-tests")]
#[wasm_bindgen]
pub async fn import_worker_contract() -> Result<(), JsValue> {
    bokheim_backend::host::web::import_worker::contract().await
}

#[cfg(feature = "web-runtime-tests")]
#[wasm_bindgen]
pub struct BookTestReader(bokheim_backend::host::web::book_source::Reader);
#[cfg(feature = "web-runtime-tests")]
#[wasm_bindgen]
impl BookTestReader {
    #[wasm_bindgen(constructor)]
    pub fn new(source: JsValue) -> Result<Self, JsValue> {
        bokheim_backend::host::web::book_source::Reader::new(source).map(Self).map_err(|error| JsValue::from_str(&error))
    }
    pub fn read(&mut self, offset: f64, length: u32) -> Result<Vec<u8>, JsValue> {
        use std::io::{Read, Seek, SeekFrom};
        self.0.seek(SeekFrom::Start(offset as u64)).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let mut bytes = Vec::new();
        (&mut self.0).take(length as u64).read_to_end(&mut bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(bytes)
    }
    pub fn cache_bytes(&self) -> usize { self.0.cache_bytes() }
}

#[cfg(feature = "web-runtime-tests")]
#[wasm_bindgen]
pub async fn import_workflow_contract(job: JsValue, io: JsValue, respond: js_sys::Function, progress: js_sys::Function) -> Result<JsValue, JsValue> {
    bokheim_backend::host::web::import_worker::workflow_contract(job, io, respond, progress).await
}
