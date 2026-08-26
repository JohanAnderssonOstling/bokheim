use epub_svelte_lib::app::{AppDataLocation, BackendContext};
use gpui::SelectedFile;
use gpui_android::AndroidApp;
use jni::EnvUnowned;
use jni::objects::{JObject, JString};
use jni::refs::Reference;
use jni::sys::{jboolean, jint, jlong};
use std::fs::File;
use std::io::Read as _;
use std::os::fd::FromRawFd as _;
use std::sync::{Mutex, OnceLock};

use crate::{UiLaunchOptions, open_ui};

static INCOMING_FILE_SENDER: OnceLock<async_channel::Sender<SelectedFile>> = OnceLock::new();
static EARLY_INCOMING_FILES: Mutex<Vec<SelectedFile>> = Mutex::new(Vec::new());

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let (incoming_sender, incoming_files) = async_channel::unbounded();
    INCOMING_FILE_SENDER.set(incoming_sender.clone()).ok();
    for file in std::mem::take(&mut *EARLY_INCOMING_FILES.lock().unwrap()) {
        incoming_sender.try_send(file).ok();
    }
    gpui_platform::android_init(app.clone());
    log::info!("Bokheim Android entry point started");

    let data_dir = app.internal_data_path().expect("Android did not provide a private application data directory");
    log::info!("initializing backend in {}", data_dir.display());
    let backend_context = BackendContext::initialize(AppDataLocation::PlatformLocal(data_dir)).expect("failed to initialize Android app-data directory");
    log::info!("backend context initialized");

    log::info!("constructing GPUI application");
    let application = gpui_platform::application().with_assets(ui_components::Assets);
    log::info!("running GPUI application");
    application.run(move |cx| {
        log::info!("opening shared Bokheim UI");
        open_ui(backend_context, UiLaunchOptions::MOBILE, Some(incoming_files), cx);
        log::info!("shared Bokheim UI opened");
    });
}

fn deliver_incoming_file(file: SelectedFile) {
    if let Some(sender) = INCOMING_FILE_SENDER.get() {
        sender.try_send(file).ok();
    } else {
        EARLY_INCOMING_FILES.lock().unwrap().push(file);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeOpenPublication<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, name: JString<'caller>, file_descriptor: jint) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            if file_descriptor < 0 {
                log::error!("Android returned an invalid incoming publication descriptor");
                return Ok(());
            }
            // SAFETY: ParcelFileDescriptor.detachFd transfers ownership of
            // this valid descriptor from Java to the native caller.
            let mut file = unsafe { File::from_raw_fd(file_descriptor) };
            let name = name.try_to_string(env)?;
            let mut bytes = Vec::new();
            match file.read_to_end(&mut bytes) {
                Ok(_) => deliver_incoming_file(SelectedFile { name, bytes }),
                Err(error) => log::error!("failed to read incoming Android publication {name}: {error}"),
            }
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeSelectedFileDescriptor<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, request_id: jlong, name: JString<'caller>, file_descriptor: jint) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            if file_descriptor < 0 {
                gpui_android::complete_file_prompt(request_id as u64, Some(anyhow::anyhow!("Android returned an invalid file descriptor")), false);
                return Ok(());
            }
            // SAFETY: ParcelFileDescriptor.detachFd transfers ownership of
            // this valid descriptor from Java to the native caller.
            let file = unsafe { File::from_raw_fd(file_descriptor) };
            let name = name.try_to_string(env)?;
            gpui_android::selected_file_descriptor(request_id as u64, name, file);
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeFilePromptResult<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, request_id: jlong, error: JString<'caller>, cancelled: jboolean) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let error = if error.is_null() { None } else { Some(anyhow::anyhow!(error.try_to_string(env)?)) };
            gpui_android::complete_file_prompt(request_id as u64, error, cancelled);
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeSelectedDirectoryRoot<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, request_id: jlong, name: JString<'caller>) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            gpui_android::selected_directory_root(request_id as u64, name.try_to_string(env)?);
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeSelectedDirectoryPath<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, request_id: jlong, path_json: JString<'caller>) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            match serde_json::from_str::<Vec<String>>(&path_json.try_to_string(env)?) {
                Ok(path) => gpui_android::selected_directory_path(request_id as u64, path),
                Err(error) => gpui_android::complete_directory_prompt(request_id as u64, Some(anyhow::anyhow!("invalid Android directory path: {error}")), false),
            }
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeSelectedDirectoryFile<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, request_id: jlong, path_json: JString<'caller>, uri: JString<'caller>) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            match serde_json::from_str::<Vec<String>>(&path_json.try_to_string(env)?) {
                Ok(path) => gpui_android::selected_directory_file(request_id as u64, path, uri.try_to_string(env)?),
                Err(error) => gpui_android::complete_directory_prompt(request_id as u64, Some(anyhow::anyhow!("invalid Android directory file path: {error}")), false),
            }
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeDirectoryPromptResult<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, request_id: jlong, error: JString<'caller>, cancelled: jboolean) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let error = if error.is_null() { None } else { Some(anyhow::anyhow!(error.try_to_string(env)?)) };
            gpui_android::complete_directory_prompt(request_id as u64, error, cancelled);
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeVolumeButton<'caller>(mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, next: jboolean) {
    unowned_env
        .with_env(|_| -> jni::errors::Result<()> {
            gpui_android::volume_button_pressed(next);
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}

#[unsafe(no_mangle)]
extern "system" fn Java_se_bokheim_reader_gpui_MainActivity_nativeWindowInsets<'caller>(
    mut unowned_env: EnvUnowned<'caller>, _activity: JObject<'caller>, safe_left: jint, safe_top: jint, safe_right: jint, safe_bottom: jint, ime_left: jint, ime_top: jint, ime_right: jint, ime_bottom: jint,
) {
    unowned_env
        .with_env(|_| -> jni::errors::Result<()> {
            gpui_android::window_insets_changed(safe_left, safe_top, safe_right, safe_bottom, ime_left, ime_top, ime_right, ime_bottom);
            Ok(())
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
}
