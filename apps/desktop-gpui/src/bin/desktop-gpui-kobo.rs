use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use app::host::{LIBRARY_MANIFEST_FILENAME, LibraryMaintenancePermit, defer_library_maintenance};
use app::{AppDataLocation, host::BackendLaunch};
use gpui::Application;
use gpui_kobo::{KoboExperienceMode, KoboPlatform, KoboPlatformOptions, KoboPowerOptions, PageButtonBehavior};

#[path = "../kobo_power.rs"]
mod kobo_power;

static LIBRARY_MAINTENANCE: Mutex<Option<LibraryMaintenancePermit>> = Mutex::new(None);

fn release_library_maintenance() {
    let permit = LIBRARY_MAINTENANCE.lock().expect("library maintenance guard poisoned").take();
    drop(permit);
    println!("KOBO_LIBRARY_MAINTENANCE phase=released_after_first_frame");
}

#[cfg(target_arch = "arm")]
fn log_bluetooth_diagnostics() {
    fn log_directory(label: &str, path: &str) {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut names = entries.filter_map(|entry| entry.ok()).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
                names.sort();
                println!("KOBO_BLUETOOTH {label} path={path} entries={:?}", names);
            }
            Err(error) => println!("KOBO_BLUETOOTH {label} path={path} error={error}"),
        }
    }

    fn log_file(label: &str, path: &str) {
        match std::fs::read_to_string(path) {
            Ok(contents) => println!("KOBO_BLUETOOTH {label} path={path} contents={:?}", contents.trim()),
            Err(error) => println!("KOBO_BLUETOOTH {label} path={path} error={error}"),
        }
    }

    fn log_command(label: &str, command: &str, arguments: &[&str]) {
        match std::process::Command::new(command).args(arguments).output() {
            Ok(output) => println!("KOBO_BLUETOOTH command={command} label={label} status={} stdout={:?} stderr={:?}", output.status, String::from_utf8_lossy(&output.stdout).trim(), String::from_utf8_lossy(&output.stderr).trim()),
            Err(error) => println!("KOBO_BLUETOOTH command={command} label={label} error={error}"),
        }
    }

    log_directory("bluetooth_class", "/sys/class/bluetooth");
    log_directory("rfkill_class", "/sys/class/rfkill");
    log_directory("network_class", "/sys/class/net");
    log_directory("firmware_rtl_bt", "/lib/firmware/rtl_bt");
    log_directory("firmware_root", "/lib/firmware");
    for path in ["/sys/class/rfkill/rfkill0/type", "/sys/class/rfkill/rfkill0/name", "/sys/class/rfkill/rfkill0/soft", "/sys/class/rfkill/rfkill0/hard"] {
        log_file("rfkill", path);
    }
    log_command("alsa_devices", "aplay", &["-l"]);
    log_command("alsa_pcms", "aplay", &["-L"]);
    log_command("connected_devices", "bluetoothctl", &["devices", "Connected"]);
    log_command("bluealsa_status", "bluetoothctl", &["info"]);

    let process_names = std::fs::read_dir("/proc")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.parse::<u32>().ok().map(|pid| (pid, entry.path().join("comm")))
        })
        .filter_map(|(pid, path)| std::fs::read_to_string(path).ok().map(|name| (pid, name.trim().to_owned())))
        .filter(|(_, name)| name.to_ascii_lowercase().contains("blue") || name.to_ascii_lowercase().contains("hci") || name.to_ascii_lowercase().contains("bt"))
        .collect::<Vec<_>>();
    println!("KOBO_BLUETOOTH processes={process_names:?}");
}

fn main() {
    #[cfg(target_arch = "arm")]
    if std::env::args().nth(1).as_deref() == Some("--wifi-power-ioctl") {
        let enabled = match std::env::args().nth(2).as_deref() {
            Some("0") => false,
            Some("1") => true,
            _ => std::process::exit(2),
        };
        let status = match kobo_device::wifi::set_radio_power(enabled) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("Kobo Wi-Fi power ioctl failed: {error}");
                1
            }
        };
        std::process::exit(status);
    }
    let startup_started = Instant::now();
    println!("KOBO_APP_STARTUP phase=process_start elapsed_ms=0");
    let args: Vec<String> = std::env::args().collect();
    let no_display = args.iter().any(|argument| argument == "--no-display");
    let auto_rotate = !args.iter().any(|argument| argument == "--no-rotation");
    let data_dir = std::env::var_os("BOKHEIM_KOBO_DATA_DIR").map(PathBuf::from).unwrap_or_else(default_data_dir);
    let backend_context = BackendLaunch::initialize(AppDataLocation::native_path(data_dir)).expect("failed to initialize Kobo app-data directory");
    println!("KOBO_APP_STARTUP phase=backend_ready elapsed_ms={}", startup_started.elapsed().as_millis());
    let libraries_dir = std::env::var_os("BOKHEIM_KOBO_LIBRARIES_DIR").map(PathBuf::from).unwrap_or_else(default_libraries_dir);
    let backend_context = backend_context.with_library_discovery(libraries_dir.clone(), "Kobo".to_owned(), fallback_library_dir(&libraries_dir)).with_libraries_after_first_frame();
    println!("KOBO_APP_STARTUP phase=libraries_ready elapsed_ms={}", startup_started.elapsed().as_millis());
    app::init_logging();
    #[cfg(target_arch = "arm")]
    if std::env::var("BOKHEIM_KOBO_BLUETOOTH_DIAGNOSTICS").as_deref() == Ok("1") {
        // bluetoothctl can wait indefinitely for firmware services. Optional
        // diagnostics must never hold up the first reader frame.
        if let Err(error) = std::thread::Builder::new().name("kobo-diagnostics".into()).spawn(log_bluetooth_diagnostics) {
            eprintln!("Could not start Kobo Bluetooth diagnostics: {error}");
        }
    }

    let platform = KoboPlatform::new(KoboPlatformOptions {
        display: !no_display,
        auto_rotate,
        interactive: !no_display,
        timeout: Duration::ZERO,
        // Browse lists may not have the page entity focused (for example
        // after opening a control), so synthetic page keys can be lost. Native
        // scrolling is handled independently of focus and works for both
        // browse lists and reader content.
        page_buttons: PageButtonBehavior::Scroll,
        power_button_handler: Some(kobo_power::suspend_kernel),
        power_button_opens_menu: true,
        power: KoboPowerOptions {
            prepare_sleep: Some(kobo_power::prepare_sleep),
            finish_sleep: Some(kobo_power::finish_sleep),
            poll: Some(kobo_power::poll),
            power_off: Some(kobo_power::power_off),
            prepare_delay: Duration::from_secs(3),
            settle_delay: Duration::from_secs(15),
            wake_confirmation_timeout: Duration::from_secs(15),
            idle_timeout: Some(Duration::from_secs(30 * 60)),
            sleep_cover: true,
        },
        wake_ui_ready_handler: Some(kobo_device::wifi::resume_after_ui),
        startup_frame_ready_handler: Some(release_library_maintenance),
        show_exit_button: false,
        experience_mode: KoboExperienceMode::LibraryFast,
        monochrome_threshold: 160,
        ..Default::default()
    })
    .expect("failed to initialize Kobo GPUI platform");
    println!("KOBO_APP_STARTUP phase=platform_ready elapsed_ms={}", startup_started.elapsed().as_millis());
    gpui_kobo::begin_application_startup_profile(startup_started).expect("failed to start Kobo application startup profile");

    *LIBRARY_MAINTENANCE.lock().expect("library maintenance guard poisoned") = Some(defer_library_maintenance());

    Application::new_inaccessible(platform.clone()).with_assets(ui_components::Assets).run(move |cx| {
        cx.set_reduce_motion(true);
        println!("KOBO_APP_STARTUP phase=ui_open_begin elapsed_ms={}", startup_started.elapsed().as_millis());
        desktop_gpui::open_ui(backend_context, desktop_gpui::UiLaunchOptions { kobo_auto_rotate: Some(auto_rotate), ..desktop_gpui::UiLaunchOptions::KOBO }, None, cx);
        println!("KOBO_APP_STARTUP phase=ui_open_complete elapsed_ms={}", startup_started.elapsed().as_millis());
    });

    if let Some(error) = platform.take_error() {
        panic!("Kobo GPUI platform failed: {error}");
    }
    if no_display {
        if let Some(frame) = platform.last_frame() {
            println!("rendered shared Bokheim UI {}x{}", frame.width(), frame.height());
        } else {
            println!("rendered shared Bokheim UI successfully before shutdown");
        }
    }
}

#[cfg(target_arch = "arm")]
fn default_data_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/.adds/bokheim")
}

#[cfg(not(target_arch = "arm"))]
fn default_data_dir() -> PathBuf {
    std::env::temp_dir().join("desktop-gpui-kobo-host")
}

fn fallback_library_dir(libraries_dir: &std::path::Path) -> PathBuf {
    std::env::var_os("BOKHEIM_KOBO_LIBRARY_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let legacy = legacy_library_dir();
        if legacy.join(LIBRARY_MANIFEST_FILENAME).is_file() { legacy } else { libraries_dir.join("default") }
    })
}

#[cfg(target_arch = "arm")]
fn default_libraries_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/Bokheim/Libraries")
}

#[cfg(not(target_arch = "arm"))]
fn default_libraries_dir() -> PathBuf {
    std::env::temp_dir().join("desktop-gpui-kobo-libraries")
}

#[cfg(target_arch = "arm")]
fn legacy_library_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/Books")
}

#[cfg(not(target_arch = "arm"))]
fn legacy_library_dir() -> PathBuf {
    std::env::temp_dir().join("desktop-gpui-kobo-books")
}
