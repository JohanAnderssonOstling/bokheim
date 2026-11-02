//! OPFS range reads for dictionary indexes and original StarDict bodies in the backend worker.
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use js_sys::{Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/dictionary_files.js")]
extern "C" {
    #[wasm_bindgen(js_name = dictionaryOpen)]
    fn open_file(id: &str, name: &str) -> Promise;
    #[wasm_bindgen(js_name = dictionaryWrite)]
    fn write_file(id: &str, name: &str, bytes: &Uint8Array) -> Promise;
    #[wasm_bindgen(js_name = dictionaryRemove)]
    fn remove_files(id: &str) -> Promise;
    #[wasm_bindgen(catch, js_name = dictionaryRead)]
    fn read_file(handle: &JsValue, bytes: &Uint8Array, offset: f64) -> Result<usize, JsValue>;
    #[wasm_bindgen(catch, js_name = dictionarySize)]
    fn file_size(handle: &JsValue) -> Result<f64, JsValue>;
    #[wasm_bindgen(catch, js_name = dictionaryClose)]
    fn close_file(handle: &JsValue) -> Result<(), JsValue>;
}

fn error(value: JsValue) -> String {
    format!("Dictionary storage: {value:?}")
}

pub struct Source {
    handle: JsValue,
    position: u64,
    size: u64,
}

impl Drop for Source {
    fn drop(&mut self) {
        let _ = close_file(&self.handle);
    }
}

impl Read for Source {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let buffer = Uint8Array::new_with_length(output.len() as u32);
        let count = read_file(&self.handle, &buffer, self.position as f64).map_err(|value| io::Error::other(error(value)))?;
        if count > output.len() {
            return Err(io::Error::other("Invalid dictionary read size"));
        }
        buffer.subarray(0, count as u32).copy_to(&mut output[..count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for Source {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::other("Invalid dictionary seek"))?;
        self.position = position;
        Ok(position)
    }
}

pub async fn open(_: &Path, id: &str, name: &str) -> Result<Source, String> {
    let handle = JsFuture::from(open_file(id, name)).await.map_err(error)?;
    let size = match file_size(&handle) {
        Ok(size) => size as u64,
        Err(value) => {
            let _ = close_file(&handle);
            return Err(error(value));
        }
    };
    Ok(Source { handle, position: 0, size })
}

pub async fn write(_: &Path, id: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
    JsFuture::from(write_file(id, name, &Uint8Array::from(bytes))).await.map_err(error)?;
    Ok(())
}

pub async fn write_all(root: &Path, id: &str, components: Vec<(String, Vec<u8>)>) -> Result<(), String> {
    for (name, bytes) in components {
        write(root, id, &name, &bytes).await?;
    }
    Ok(())
}

pub async fn remove(_: &Path, id: &str) -> Result<(), String> {
    JsFuture::from(remove_files(id)).await.map_err(error)?;
    Ok(())
}
