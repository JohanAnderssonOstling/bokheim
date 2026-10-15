//! Runtime dispatch for the upstream offline TTS C ABI. No native symbols are
//! linked into the host; the packaged library is opened on first model load.

use super::{OfflineTtsConfig, SherpaOnnxGeneratedAudio, SherpaOnnxGeneratedAudioProgressCallbackWithArg, SherpaOnnxGenerationConfig, SherpaOnnxOfflineTts};
use libloading::Library;
use std::ffi::{c_char, c_void, OsStr};
use std::ptr;
use std::sync::OnceLock;

type Create = unsafe extern "C" fn(*const OfflineTtsConfig) -> *const SherpaOnnxOfflineTts;
type Destroy = unsafe extern "C" fn(*const SherpaOnnxOfflineTts);
type ModelProperty = unsafe extern "C" fn(*const SherpaOnnxOfflineTts) -> i32;
type Generate = unsafe extern "C" fn(*const SherpaOnnxOfflineTts, *const c_char, *const SherpaOnnxGenerationConfig, SherpaOnnxGeneratedAudioProgressCallbackWithArg, *mut c_void) -> *const SherpaOnnxGeneratedAudio;
type DestroyAudio = unsafe extern "C" fn(*const SherpaOnnxGeneratedAudio);

struct Api {
    // Every function pointer, model and generated audio allocation borrows this
    // library. The successful API is retained for the entire process lifetime.
    _library: Library,
    create: Create,
    destroy: Destroy,
    sample_rate: ModelProperty,
    num_speakers: ModelProperty,
    generate: Generate,
    destroy_audio: DestroyAudio,
}

impl Api {
    /// The library and declarations must have the matching Sherpa 1.13.8 ABI.
    unsafe fn open(path: &OsStr) -> Result<Self, String> {
        let library = unsafe { Library::new(path) }.map_err(|error| format!("Could not load the speech runtime: {error}"))?;
        // Copy the typed pointers while retaining the owning library below.
        unsafe {
            Ok(Self {
                create: symbol(&library, b"SherpaOnnxCreateOfflineTts\0")?,
                destroy: symbol(&library, b"SherpaOnnxDestroyOfflineTts\0")?,
                sample_rate: symbol(&library, b"SherpaOnnxOfflineTtsSampleRate\0")?,
                num_speakers: symbol(&library, b"SherpaOnnxOfflineTtsNumSpeakers\0")?,
                generate: symbol(&library, b"SherpaOnnxOfflineTtsGenerateWithConfig\0")?,
                destroy_audio: symbol(&library, b"SherpaOnnxDestroyOfflineTtsGeneratedAudio\0")?,
                _library: library,
            })
        }
    }
}

unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
    unsafe { library.get::<T>(name) }.map(|symbol| *symbol).map_err(|error| format!("The speech runtime is missing a required function: {error}"))
}

static API: OnceLock<Api> = OnceLock::new();

fn load() -> Result<&'static Api, String> {
    if let Some(api) = API.get() {
        return Ok(api);
    }
    // Android's app linker namespace searches the APK's packaged native
    // libraries and resolves Sherpa's ONNX Runtime dependency at this point.
    let api = unsafe { Api::open(OsStr::new("libsherpa-onnx-c-api.so")) }?;
    match API.set(api) {
        Ok(()) => {}
        // A concurrent successful loader owns the retained handle. Dropping
        // this duplicate releases only our extra reference to the library.
        Err(_duplicate) => {}
    }
    Ok(loaded())
}

fn loaded() -> &'static Api {
    API.get().expect("successful TTS model creation initializes the runtime")
}

/// Resolve all TTS functions before model creation, returning useful loader
/// errors to the host. Failures are not cached, so a later attempt can retry.
pub fn initialize_tts() -> Result<(), String> {
    load().map(|_| ())
}

/// Uses the upstream C API's null-on-failure model creation contract.
pub unsafe fn SherpaOnnxCreateOfflineTts(config: *const OfflineTtsConfig) -> *const SherpaOnnxOfflineTts {
    match load() {
        Ok(api) => unsafe { (api.create)(config) },
        Err(_) => ptr::null(),
    }
}

pub unsafe fn SherpaOnnxDestroyOfflineTts(tts: *const SherpaOnnxOfflineTts) {
    unsafe { (loaded().destroy)(tts) }
}

pub unsafe fn SherpaOnnxOfflineTtsSampleRate(tts: *const SherpaOnnxOfflineTts) -> i32 {
    unsafe { (loaded().sample_rate)(tts) }
}

pub unsafe fn SherpaOnnxOfflineTtsNumSpeakers(tts: *const SherpaOnnxOfflineTts) -> i32 {
    unsafe { (loaded().num_speakers)(tts) }
}

pub unsafe fn SherpaOnnxOfflineTtsGenerateWithConfig(
    tts: *const SherpaOnnxOfflineTts, text: *const c_char, config: *const SherpaOnnxGenerationConfig, callback: SherpaOnnxGeneratedAudioProgressCallbackWithArg, arg: *mut c_void,
) -> *const SherpaOnnxGeneratedAudio {
    unsafe { (loaded().generate)(tts, text, config, callback, arg) }
}

pub unsafe fn SherpaOnnxDestroyOfflineTtsGeneratedAudio(audio: *const SherpaOnnxGeneratedAudio) {
    unsafe { (loaded().destroy_audio)(audio) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_returns_a_loader_error_without_initializing_the_api() {
        let error = unsafe { Api::open(OsStr::new("bokheim-missing-sherpa-runtime.so")) }.err().expect("missing library must fail");
        assert!(error.contains("Could not load the speech runtime"), "{error}");
        assert!(API.get().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn incompatible_library_returns_a_symbol_error_without_initializing_the_api() {
        let error = unsafe { Api::open(OsStr::new("libc.so.6")) }.err().expect("non-Sherpa library must fail");
        assert!(error.contains("SherpaOnnxCreateOfflineTts"), "{error}");
        assert!(API.get().is_none());
    }
}
