//! Logical sleep owns peripherals; the GPUI backend classifies physical wakes.
use gpui_kobo::KoboPowerEvent;
use kobo_device::{
    battery,
    power::{BatteryAction, SleepSession, battery_action},
};
use std::cell::RefCell;
use std::time::{Duration, Instant};

const AUTOMATIC_POWER_OFF: Duration = Duration::from_secs(3 * 24 * 60 * 60);
const BATTERY_POLL_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct PowerState {
    sleep: Option<SleepSession>,
    next_battery_poll: Option<Instant>,
    low_battery_warned: bool,
}

thread_local! {
    static STATE: RefCell<PowerState> = RefCell::new(PowerState::default());
}

pub fn prepare_sleep() -> Result<(), String> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.sleep.is_some() {
            return Err("Sleep session already active".into());
        }
        state.sleep = Some(SleepSession::prepare(Some(AUTOMATIC_POWER_OFF)).map_err(|error| error.to_string())?);
        Ok(())
    })
}

pub fn suspend_kernel() -> Result<(), String> {
    kobo_device::suspend_kernel().map_err(|error| error.to_string())
}

pub fn finish_sleep() -> Result<(), String> {
    STATE.with(|state| state.borrow_mut().sleep.take().map_or(Ok(()), |sleep| sleep.finish().map_err(|error| error.to_string())))
}

pub fn poll() -> KoboPowerEvent {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.sleep.as_ref().is_some_and(SleepSession::automatic_power_off_due) {
            return KoboPowerEvent::PowerOff;
        }
        let now = Instant::now();
        if state.next_battery_poll.is_some_and(|deadline| now < deadline) {
            return KoboPowerEvent::None;
        }
        state.next_battery_poll = Some(now + BATTERY_POLL_INTERVAL);
        let Ok(status) = battery::read() else { return KoboPowerEvent::None };
        match battery_action(&status) {
            BatteryAction::PowerOff => KoboPowerEvent::PowerOff,
            BatteryAction::Warn(percent) if !state.low_battery_warned => {
                state.low_battery_warned = true;
                KoboPowerEvent::Warning(format!("Battery low ({percent}%). Connect a charger."))
            }
            BatteryAction::Warn(_) => KoboPowerEvent::None,
            BatteryAction::None => {
                state.low_battery_warned = false;
                KoboPowerEvent::None
            }
        }
    })
}

pub fn power_off() -> Result<(), String> {
    let path = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE").ok_or_else(|| "Kobo launcher must be updated before powering off".to_owned())?;
    std::fs::write(path, "poweroff").map_err(|error| error.to_string())?;
    STATE.with(|state| {
        if let Some(sleep) = state.borrow_mut().sleep.take() {
            sleep.power_off();
        }
    });
    Ok(())
}
