//! Incremental hashing beside the import worker's browser file I/O.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ImportHasher(blake3::Hasher);

#[wasm_bindgen]
impl ImportHasher {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn finalize(&self) -> String {
        self.0.finalize().to_hex().to_string()
    }
}
