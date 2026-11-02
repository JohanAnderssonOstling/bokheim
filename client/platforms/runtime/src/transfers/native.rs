#[cfg(not(target_os = "android"))]
use super::NetworkCost;
use super::POLICY;
use crate::executor::BackendExecutor;
#[cfg(not(target_os = "android"))]
use crate::executor::BackendTask;
#[cfg(not(target_os = "android"))]
use crate::storage_queue::{self, Priority};
#[cfg(not(target_os = "android"))]
use std::future::pending;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
#[cfg(not(target_os = "android"))]
use std::time::Duration;

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(not(target_os = "android"))]
static MONITOR: Mutex<Option<BackendTask<()>>> = Mutex::new(None);

/// The registry owns the preference; this small atomic mirror lets the browser
/// helper and native update workers observe the same setting across processes.
pub fn initialize(directory: &Path, allow: bool, executor: &BackendExecutor) -> io::Result<()> {
    *FILE.lock().unwrap_or_else(PoisonError::into_inner) = Some(directory.join("transfer-policy.json"));
    save_allow_metered(allow)?;
    start(executor, false);
    Ok(())
}

pub fn configure_reader(path: PathBuf, executor: &BackendExecutor) {
    *FILE.lock().unwrap_or_else(PoisonError::into_inner) = Some(path);
    refresh();
    start(executor, true);
}

pub fn save_allow_metered(allow: bool) -> io::Result<()> {
    let file = FILE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(path) = file.as_ref() {
        crate::file_storage::write_atomic_sync(path, if allow { b"true" } else { b"false" })?;
    }
    POLICY.set_allow_metered(allow);
    Ok(())
}

fn refresh() {
    if let Some(path) = FILE.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        if let Ok(value) = fs::read_to_string(path) {
            match value.trim() {
                "true" => POLICY.set_allow_metered(true),
                "false" => POLICY.set_allow_metered(false),
                _ => {}
            }
        }
    }
}

// Android publishes observations through platform callbacks.
#[cfg(target_os = "android")]
fn start(_executor: &BackendExecutor, _refresh_preferences: bool) {}

#[cfg(not(target_os = "android"))]
fn start(executor: &BackendExecutor, refresh_preferences: bool) {
    let mut monitor = MONITOR.lock().unwrap_or_else(PoisonError::into_inner);
    if monitor.is_none() {
        POLICY.cost.store(3, std::sync::atomic::Ordering::Release);
        POLICY.notify();
    }
    #[cfg(target_os = "macos")]
    macos::start();
    let task = executor.spawn(async move {
        let preferences = async move {
            if !refresh_preferences {
                return pending::<()>().await;
            }
            loop {
                if let Err(error) = storage_queue::run(Priority::Background, refresh).await {
                    log::warn!("could not refresh transfer preferences: {error}");
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        };
        tokio::select! {
            () = preferences => {},
            () = monitor_network_cost() => {},
        }
    });
    // Replacement cancels the old monitor; a replacement backend/runtime can
    // start a new one without retaining a process-lifetime OS thread.
    *monitor = Some(task);
}

#[cfg(all(target_os = "linux", feature = "linux-network-cost"))]
async fn monitor_network_cost() {
    loop {
        if let Err(error) = watch_network_cost().await {
            POLICY.set_network_cost(NetworkCost::Unknown);
            log::debug!("network cost observation will reconnect: {error}");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(all(target_os = "linux", feature = "linux-network-cost"))]
async fn watch_network_cost() -> zbus::Result<()> {
    use futures_util::StreamExt;
    use zbus::{MessageStream, Proxy, connection::Builder};

    let connection = Builder::system()?
        .internal_executor(false)
        .method_timeout(Duration::from_secs(1))
        .build()
        .await?;
    let driver = connection.executor().clone();
    let observe = async {
        let proxy = Proxy::new(&connection, "org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager", "org.freedesktop.NetworkManager").await?;
        let mut changes = proxy.receive_property_changed::<u32>("Metered").await;
        let mut owners = proxy.receive_owner_changed().await?;
        let mut messages = MessageStream::from(&connection);
        POLICY.set_network_cost(network_cost(proxy.get_property::<u32>("Metered").await?));
        loop {
            tokio::select! {
                change = changes.next() => {
                    let Some(change) = change else { return Err(zbus::Error::Failure("network cost subscription closed".into())) };
                    POLICY.set_network_cost(network_cost(change.get().await?));
                }
                _ = owners.next() => return Err(zbus::Error::Failure("NetworkManager owner changed".into())),
                message = messages.next() => match message {
                    Some(Ok(_)) => {},
                    Some(Err(error)) => return Err(error),
                    None => return Err(zbus::Error::Failure("system bus disconnected".into())),
                },
            }
        }
    };
    // Drive zbus on the existing backend runtime instead of allowing it to
    // create an additional connection executor thread.
    tokio::select! {
        result = observe => result,
        () = async { loop { driver.tick().await; } } => unreachable!(),
    }
}

#[cfg(all(target_os = "linux", feature = "linux-network-cost"))]
fn network_cost(metered: u32) -> NetworkCost {
    match metered {
        1 | 3 => NetworkCost::Metered,
        2 | 4 => NetworkCost::Unmetered,
        _ => NetworkCost::Unknown,
    }
}

#[cfg(target_os = "windows")]
async fn monitor_network_cost() {
    loop {
        let cost = crate::executor::run_blocking(platform_cost).await.unwrap_or(NetworkCost::Unknown);
        POLICY.set_network_cost(cost);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(target_os = "macos")]
async fn monitor_network_cost() {
    // Network.framework delivers changes on its dispatch queue.
    pending::<()>().await;
}

#[cfg(not(any(target_os = "android", target_os = "windows", target_os = "macos", all(target_os = "linux", feature = "linux-network-cost"))))]
async fn monitor_network_cost() {
    POLICY.set_network_cost(NetworkCost::Unknown);
    pending::<()>().await;
}

#[cfg(target_os = "windows")]
fn platform_cost() -> NetworkCost {
    use windows::Win32::Networking::NetworkListManager::{
        INetworkCostManager, NetworkListManager, NLM_CONNECTION_COST_FIXED, NLM_CONNECTION_COST_OVERDATALIMIT, NLM_CONNECTION_COST_ROAMING, NLM_CONNECTION_COST_UNRESTRICTED, NLM_CONNECTION_COST_VARIABLE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};
    // This runs on a shared blocking worker, outside the UI's apartment.
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return NetworkCost::Unknown;
        }
        let cost = CoCreateInstance::<_, INetworkCostManager>(&NetworkListManager, None, CLSCTX_ALL).and_then(|manager| {
            let mut value = 0;
            manager.GetCost(&mut value, std::ptr::null())?;
            Ok(value)
        });
        CoUninitialize();
        match cost {
            Ok(value) if value & (NLM_CONNECTION_COST_FIXED.0 | NLM_CONNECTION_COST_VARIABLE.0 | NLM_CONNECTION_COST_ROAMING.0 | NLM_CONNECTION_COST_OVERDATALIMIT.0) as u32 != 0 => NetworkCost::Metered,
            Ok(value) if value & NLM_CONNECTION_COST_UNRESTRICTED.0 as u32 != 0 => NetworkCost::Unmetered,
            _ => NetworkCost::Unknown,
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::{Block, RcBlock};
    use dispatch2::DispatchQueue;
    use std::ffi::c_void;
    use std::sync::Once;

    static START: Once = Once::new();

    #[link(name = "Network", kind = "framework")]
    extern "C" {
        fn nw_path_monitor_create() -> *mut c_void;
        fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: &DispatchQueue);
        fn nw_path_monitor_set_update_handler(monitor: *mut c_void, handler: &Block<dyn Fn(*mut c_void)>);
        fn nw_path_monitor_start(monitor: *mut c_void);
        fn nw_path_is_expensive(path: *mut c_void) -> bool;
        fn nw_path_is_constrained(path: *mut c_void) -> bool;
    }

    pub(super) fn start() {
        START.call_once(start_monitor);
    }

    fn start_monitor() {
        let queue = DispatchQueue::new("se.bokheim.network-cost", None);
        let handler = RcBlock::new(|path: *mut c_void| {
            // Network.framework lends a valid path during this callback.
            let metered = unsafe { nw_path_is_expensive(path) || nw_path_is_constrained(path) };
            POLICY.set_network_cost(if metered { NetworkCost::Metered } else { NetworkCost::Unmetered });
        });
        // The process owns one monitor for its lifetime; Network.framework
        // retains the queue and copies the handler when registering them.
        unsafe {
            let monitor = nw_path_monitor_create();
            if monitor.is_null() {
                return;
            }
            nw_path_monitor_set_queue(monitor, &queue);
            nw_path_monitor_set_update_handler(monitor, &handler);
            nw_path_monitor_start(monitor);
        }
    }
}
