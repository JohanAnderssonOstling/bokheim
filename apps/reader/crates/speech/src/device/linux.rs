//! Optional local eSpeak installation. Dynamic loading preserves the Linux ABI baseline.
use super::{Audio, DeviceVoice};
use libloading::Library;
use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, OnceLock};

#[repr(C)]
struct Voice {
    name: *const c_char,
    languages: *const c_char,
    identifier: *const c_char,
    gender: u8,
    age: u8,
    variant: u8,
    reserved: u8,
    score: c_int,
    spare: *mut c_void,
}
type Callback = unsafe extern "C" fn(*mut i16, c_int, *mut c_void) -> c_int;
struct Engine {
    _library: Library,
    list: unsafe extern "C" fn(*const Voice) -> *const *const Voice,
    current: unsafe extern "C" fn() -> *const Voice,
    select: unsafe extern "C" fn(*const c_char) -> c_int,
    parameter: unsafe extern "C" fn(c_int, c_int, c_int) -> c_int,
    callback: unsafe extern "C" fn(Option<Callback>),
    synth: unsafe extern "C" fn(*const c_void, usize, u32, c_int, u32, u32, *mut u32, *mut c_void) -> c_int,
    rate: u32,
}

static ENGINE: OnceLock<Mutex<Option<Engine>>> = OnceLock::new();
thread_local! { static REQUEST: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) }; }

impl Engine {
    fn load() -> Result<Self, String> {
        // This engine only reads installed local voice data; it has no network backend.
        let library = ["libespeak-ng.so.1", "libespeak.so.1"].into_iter().find_map(|name| unsafe { Library::new(name).ok() }).ok_or("No local device speech engine is installed. Install eSpeak NG or choose a Bokheim voice.")?;
        unsafe {
            let initialize = library.get::<unsafe extern "C" fn(c_int, c_int, *const c_char, c_int) -> c_int>(b"espeak_Initialize\0").map_err(|error| error.to_string())?;
            // Synchronous retrieval, never engine-owned playback; missing data must not exit the app.
            let rate = initialize(2, 0, std::ptr::null(), 0x8000);
            if rate <= 0 {
                return Err("The local speech engine has no installed voice data.".into());
            }
            Ok(Self {
                list: *library.get(b"espeak_ListVoices\0").map_err(|error| error.to_string())?,
                current: *library.get(b"espeak_GetCurrentVoice\0").map_err(|error| error.to_string())?,
                select: *library.get(b"espeak_SetVoiceByName\0").map_err(|error| error.to_string())?,
                parameter: *library.get(b"espeak_SetParameter\0").map_err(|error| error.to_string())?,
                callback: *library.get(b"espeak_SetSynthCallback\0").map_err(|error| error.to_string())?,
                synth: *library.get(b"espeak_Synth\0").map_err(|error| error.to_string())?,
                rate: rate as u32,
                _library: library,
            })
        }
    }
    fn voices(&self) -> Vec<DeviceVoice> {
        let mut result = Vec::new();
        unsafe {
            let list = (self.list)(std::ptr::null());
            if list.is_null() {
                return result;
            }
            for index in 0..4096 {
                let voice = *list.add(index);
                if voice.is_null() {
                    break;
                }
                let voice = &*voice;
                if voice.name.is_null() || voice.identifier.is_null() || voice.languages.is_null() || *voice.languages == 0 {
                    continue;
                }
                let name = CStr::from_ptr(voice.name).to_string_lossy().into_owned();
                let id = format!("espeak:{}", CStr::from_ptr(voice.identifier).to_string_lossy());
                let language = CStr::from_ptr(voice.languages.add(1)).to_string_lossy().into_owned();
                if !name.is_empty() && !language.is_empty() {
                    result.push(DeviceVoice { id, name, language });
                }
            }
        }
        result
    }
}

fn with_engine<T>(action: impl FnOnce(&Engine) -> Result<T, String>) -> Result<T, String> {
    let mut engine = ENGINE.get_or_init(|| Mutex::new(None)).lock().map_err(|_| "The local speech engine was interrupted. Restart the app to retry.")?;
    if engine.is_none() {
        *engine = Some(Engine::load()?);
    }
    action(engine.as_ref().expect("engine was loaded"))
}

pub(super) fn voices() -> Result<Vec<DeviceVoice>, String> {
    with_engine(|engine| Ok(engine.voices()))
}

struct Request<C> {
    samples: Vec<f32>,
    cancelled: C,
    limit: usize,
    failed: bool,
}
unsafe extern "C" fn collect<C: Fn() -> bool>(audio: *mut i16, count: c_int, _: *mut c_void) -> c_int {
    // eSpeak's synchronous retrieval calls back on this thread, only before Synth returns.
    REQUEST.with(|slot| {
        let pointer = slot.get().cast::<Request<C>>();
        if pointer.is_null() {
            return 1;
        }
        let request = unsafe { &mut *pointer };
        catch_unwind(AssertUnwindSafe(|| {
            if (request.cancelled)() {
                return 1;
            }
            if count < 0 || request.samples.len().saturating_add(count as usize) > request.limit {
                request.failed = true;
                return 1;
            }
            if !audio.is_null() && count > 0 {
                request.samples.extend(unsafe { std::slice::from_raw_parts(audio, count as usize) }.iter().map(|sample| f32::from(*sample) / 32768.0));
            }
            0
        }))
        .unwrap_or_else(|_| {
            request.failed = true;
            1
        })
    })
}

pub(super) fn synthesize<C: Fn() -> bool>(id: &str, text: &str, rate: f32, cancelled: C, mut emit: impl FnMut(Audio) -> bool) -> Result<(), String> {
    with_engine(|engine| {
        let voice = engine.voices().into_iter().find(|voice| voice.id == id).ok_or("The selected local voice is no longer installed. Choose another local voice.")?;
        let name = CString::new(voice.name).map_err(|_| "Invalid installed voice name.")?;
        let text = CString::new(text).map_err(|_| "The reading sample contains an invalid null character.")?;
        if cancelled() {
            return Err("Cancelled".into());
        }
        let mut request = Request { samples: Vec::new(), cancelled, limit: engine.rate as usize * 90, failed: false };
        unsafe {
            if (engine.select)(name.as_ptr()) != 0 || (engine.parameter)(1, (175.0 * rate).clamp(80.0, 450.0) as c_int, 0) != 0 {
                return Err("The installed local voice could not be selected.".into());
            }
            let current = (engine.current)();
            if current.is_null() || (*current).identifier.is_null() || format!("espeak:{}", CStr::from_ptr((*current).identifier).to_string_lossy()) != id {
                return Err("The installed engine substituted another voice. Choose another local voice.".into());
            }
            REQUEST.with(|slot| slot.set((&mut request as *mut Request<C>).cast()));
            (engine.callback)(Some(collect::<C>));
            let result = (engine.synth)(text.as_ptr().cast(), text.as_bytes_with_nul().len(), 0, 1, 0, 1, std::ptr::null_mut(), std::ptr::null_mut());
            (engine.callback)(None);
            REQUEST.with(|slot| slot.set(std::ptr::null_mut()));
            if (request.cancelled)() {
                return Err("Cancelled".into());
            }
            if result != 0 || request.failed || request.samples.is_empty() {
                return Err("The local voice could not generate this reading sample.".into());
            }
        }
        if emit(Audio { samples: request.samples, sample_rate: engine.rate, last: true }) { Ok(()) } else { Err("Cancelled".into()) }
    })
}
