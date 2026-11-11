use std::array::from_fn;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use app::{AppDataLocation, host::BackendLaunch};
use gpui::{Application, px, size};
use gpui_kobo::{KoboExperienceMode, KoboInputProfile, KoboPlatform, KoboPlatformOptions, KoboPowerOptions, KoboTouchProtocol, PageButtonBehavior, TouchTransform};
use kobo_device::profile::{Profile, TouchProtocol};

#[path = "../kobo_power.rs"]
mod kobo_power;

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

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
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
    let install_root = std::env::var_os("BOKHEIM_KOBO_INSTALL_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/mnt/onboard"));
    kobo_update::initialize(install_root, env!("CARGO_PKG_VERSION")).map_err(|error| error.to_string())?;
    if std::env::args().nth(1).as_deref() == Some("--apply-update") {
        return kobo_update::installation().and_then(|installation| installation.apply()).map_err(|error| error.to_string());
    }
    let profile = kobo_device::profile::current().map_err(|error| error.to_string())?;
    let auto_rotate = profile.has_gyroscope() && !args.iter().any(|argument| argument == "--no-rotation");
    println!("KOBO_DEVICE model={} product={} name={:?} touch={:?}", profile.id, profile.product, profile.name, profile.touch);
    if args.iter().any(|argument| argument == "--check-device") {
        return Ok(());
    }
    let data_dir = std::env::var_os("BOKHEIM_KOBO_DATA_DIR").map(PathBuf::from).unwrap_or_else(default_data_dir);
    let backend_context = BackendLaunch::initialize(AppDataLocation::native_path(data_dir)).map_err(|error| format!("failed to initialize Kobo app-data directory: {error}"))?;
    let libraries_dir = std::env::var_os("BOKHEIM_KOBO_LIBRARIES_DIR").map(PathBuf::from).unwrap_or_else(default_libraries_dir);
    let backend_context = backend_context.with_library_discovery(libraries_dir.clone(), "Kobo".to_owned(), fallback_library_dir(&libraries_dir));
    app::init_logging();
    #[cfg(target_arch = "arm")]
    if std::env::var("BOKHEIM_KOBO_BLUETOOTH_DIAGNOSTICS").as_deref() == Ok("1") {
        // bluetoothctl can wait indefinitely for firmware services. Optional
        // diagnostics must never hold up the first reader frame.
        if let Err(error) = std::thread::Builder::new().name("kobo-diagnostics".into()).spawn(log_bluetooth_diagnostics) {
            eprintln!("Could not start Kobo Bluetooth diagnostics: {error}");
        }
    }

    // Keep physical control and text sizes consistent across 167–300 DPI panels.
    let scale_factor = f32::from(profile.dpi) / 150.0;
    let platform = KoboPlatform::new(KoboPlatformOptions {
        input_profile: input_profile(profile),
        logical_size: size(px(f32::from(profile.width) / scale_factor), px(f32::from(profile.height) / scale_factor)),
        scale_factor,
        auto_rotate,
        timeout: Duration::ZERO,
        // Browse lists may not have the page entity focused (for example
        // after opening a control), so synthetic page keys can be lost. Native
        // scrolling is handled independently of focus and works for both
        // browse lists and reader content.
        page_buttons: PageButtonBehavior::Scroll,
        power_button_handler: Some(kobo_power::suspend_kernel),
        power_button_opens_menu: false,
        power: KoboPowerOptions {
            prepare_sleep: Some(kobo_power::prepare_sleep),
            finish_sleep: Some(kobo_power::finish_sleep),
            poll: Some(kobo_power::poll),
            power_off: Some(kobo_power::power_off),
            prepare_delay: Duration::from_secs(3),
            settle_delay: Duration::from_secs(15),
            wake_confirmation_timeout: Duration::from_secs(2),
            idle_timeout: kobo_device::settings::idle_timeout(),
            idle_timeout_provider: Some(kobo_device::settings::idle_timeout),
            idle_power_off_timeout_provider: Some(kobo_device::settings::power_off_timeout),
            sleep_cover: true,
        },
        wake_ui_ready_handler: Some(kobo_device::wifi::resume_after_ui),
        show_exit_button: false,
        experience_mode: KoboExperienceMode::LibraryFast,
        monochrome_threshold: 160,
        ..Default::default()
    })
    .map_err(|error| format!("failed to initialize Kobo GPUI platform: {error}"))?;

    let opened = Rc::new(Cell::new(false));
    let window_opened = opened.clone();
    Application::new_inaccessible(platform.clone()).with_assets(ui_components::Assets).run(move |cx| {
        cx.set_global(ui_components::kobo::DeviceProfile(profile));
        cx.set_reduce_motion(true);
        window_opened.set(desktop_gpui::open_ui(backend_context, desktop_gpui::UiLaunchOptions { kobo_auto_rotate: Some(auto_rotate), ..desktop_gpui::UiLaunchOptions::KOBO }, None, cx).is_some());
    });

    if let Some(error) = platform.take_error() {
        return Err(format!("Kobo GPUI platform failed: {error}"));
    }
    if !opened.get() {
        return Err("failed to open the Kobo application UI".into());
    }
    Ok(())
}

fn input_profile(profile: &Profile) -> KoboInputProfile {
    KoboInputProfile {
        protocol: match profile.touch {
            TouchProtocol::Single => KoboTouchProtocol::Single,
            TouchProtocol::MultitouchA => KoboTouchProtocol::MultitouchA,
            TouchProtocol::MultitouchB => KoboTouchProtocol::MultitouchB,
            TouchProtocol::MultitouchC => KoboTouchProtocol::MultitouchC,
        },
        touch_transforms: Some(from_fn(|rotation| {
            let transform = profile.touch_transform(rotation as u8);
            TouchTransform { swap_axes: transform.swap_axes, invert_x: transform.invert_x, invert_y: transform.invert_y }
        })),
        gyroscope_rotations: profile.has_gyroscope().then(|| from_fn(|sensor| profile.gyroscope_rotation(sensor as u8))),
        invert_page_buttons: from_fn(|rotation| profile.invert_page_buttons(rotation as u8)),
    }
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/.adds/bokheim")
}

fn fallback_library_dir(libraries_dir: &std::path::Path) -> PathBuf {
    std::env::var_os("BOKHEIM_KOBO_LIBRARY_DIR").map(PathBuf::from).unwrap_or_else(|| libraries_dir.join("default"))
}

fn default_libraries_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/Bokheim/Libraries")
}
