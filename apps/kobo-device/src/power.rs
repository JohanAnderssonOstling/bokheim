//! Peripherals remain asleep across unsolicited kernel wakes.
use crate::{battery::BatteryStatus, brightness, natural_light, rtc::Alarm, wifi};
use std::io::{self, ErrorKind};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
struct LightLevels {
    intensity: u8,
    warmth: Option<u8>,
}

trait Frontlight {
    fn read(&mut self) -> io::Result<Option<LightLevels>>;
    fn intensity(&mut self, percent: u8) -> io::Result<()>;
    fn warmth(&mut self, percent: u8) -> io::Result<()>;
}

struct DeviceFrontlight;
impl Frontlight for DeviceFrontlight {
    fn read(&mut self) -> io::Result<Option<LightLevels>> {
        let intensity = match brightness::read() {
            Ok(value) => value,
            Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::Unsupported) => return Ok(None),
            Err(error) => return Err(error),
        };
        let warmth = match natural_light::read() {
            Ok(value) => value,
            // An unidentified color scale must not be changed.
            Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::Unsupported) => None,
            Err(error) => return Err(error),
        };
        Ok(Some(LightLevels { intensity, warmth }))
    }
    fn intensity(&mut self, percent: u8) -> io::Result<()> {
        brightness::set(percent).map(|_| ())
    }
    fn warmth(&mut self, percent: u8) -> io::Result<()> {
        natural_light::set(percent).map(|_| ())
    }
}

fn restore_lights(light: &mut impl Frontlight, levels: LightLevels) -> io::Result<()> {
    let warmth = levels.warmth.map_or(Ok(()), |value| light.warmth(value));
    // Attempt brightness restoration even when the color driver fails.
    let intensity = light.intensity(levels.intensity);
    warmth.and(intensity)
}

fn dim_lights(light: &mut impl Frontlight, levels: Option<LightLevels>) -> io::Result<()> {
    let Some(levels) = levels else { return Ok(()) };
    let result = light.intensity(0).and_then(|_| levels.warmth.map_or(Ok(()), |_| light.warmth(0)));
    if result.is_err() {
        if let Err(error) = restore_lights(light, levels) {
            eprintln!("Kobo frontlight rollback failed: {error}");
        }
    }
    result
}

pub struct SleepSession {
    lights: Option<LightLevels>,
    _wifi: wifi::SleepGuard,
    alarm: Option<Alarm>,
}

impl SleepSession {
    pub fn prepare(auto_power_off: Option<Duration>) -> io::Result<Self> {
        let levels = DeviceFrontlight.read()?;
        let wifi = wifi::prepare_sleep().map_err(io::Error::other)?;
        dim_lights(&mut DeviceFrontlight, levels)?;
        let alarm = auto_power_off.and_then(|duration| match Alarm::arm(duration) {
            Ok(alarm) => Some(alarm),
            Err(error) => {
                eprintln!("Kobo RTC automatic power-off unavailable: {error}");
                None
            }
        });
        Ok(Self { lights: levels, _wifi: wifi, alarm })
    }

    pub fn automatic_power_off_due(&self) -> bool {
        self.alarm.as_ref().is_some_and(Alarm::due)
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.lights.take().map_or(Ok(()), |levels| restore_lights(&mut DeviceFrontlight, levels))
    }

    /// Preserve the unlit screen when the sleeping device is being shut down.
    pub fn power_off(mut self) {
        self.lights = None;
    }
}

impl Drop for SleepSession {
    fn drop(&mut self) {
        if let Some(levels) = self.lights.take() {
            if let Err(error) = restore_lights(&mut DeviceFrontlight, levels) {
                eprintln!("Kobo frontlight recovery failed: {error}");
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryAction {
    None,
    Warn(u8),
    PowerOff,
}

pub fn battery_action(status: &BatteryStatus) -> BatteryAction {
    if matches!(status.status.as_str(), "Charging" | "Full" | "Unknown") {
        return BatteryAction::None;
    }
    if status.percent < 3 {
        BatteryAction::PowerOff
    } else if status.percent < 10 {
        BatteryAction::Warn(status.percent)
    } else {
        BatteryAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Light {
        levels: LightLevels,
        calls: Vec<(bool, u8)>,
        fail_color: bool,
    }
    impl Frontlight for Light {
        fn read(&mut self) -> io::Result<Option<LightLevels>> {
            Ok(Some(self.levels))
        }
        fn intensity(&mut self, percent: u8) -> io::Result<()> {
            self.calls.push((false, percent));
            self.levels.intensity = percent;
            Ok(())
        }
        fn warmth(&mut self, percent: u8) -> io::Result<()> {
            self.calls.push((true, percent));
            if self.fail_color {
                return Err(io::Error::other("color failure"));
            }
            self.levels.warmth = Some(percent);
            Ok(())
        }
    }

    #[test]
    fn lights_are_dimmed_and_restored_in_driver_order() {
        let levels = LightLevels { intensity: 37, warmth: Some(60) };
        let mut light = Light { levels, calls: vec![], fail_color: false };
        dim_lights(&mut light, Some(levels)).unwrap();
        assert_eq!(light.levels, LightLevels { intensity: 0, warmth: Some(0) });
        restore_lights(&mut light, levels).unwrap();
        assert_eq!(light.levels, levels);
        assert_eq!(light.calls, [(false, 0), (true, 0), (true, 60), (false, 37)]);
    }

    #[test]
    fn failed_color_changes_still_restore_brightness() {
        let levels = LightLevels { intensity: 37, warmth: Some(60) };
        let mut light = Light { levels, calls: vec![], fail_color: true };
        assert!(dim_lights(&mut light, Some(levels)).is_err());
        assert_eq!(light.levels.intensity, 37);
        assert!(restore_lights(&mut light, levels).is_err());
        assert_eq!(light.calls.last(), Some(&(false, 37)));
    }

    #[test]
    fn charging_and_unknown_battery_status_never_request_shutdown() {
        for status in ["Charging", "Full", "Unknown"] {
            assert_eq!(battery_action(&BatteryStatus { percent: 0, status: status.into() }), BatteryAction::None);
        }
        assert_eq!(battery_action(&BatteryStatus { percent: 2, status: "Discharging".into() }), BatteryAction::PowerOff);
        assert_eq!(battery_action(&BatteryStatus { percent: 9, status: "Discharging".into() }), BatteryAction::Warn(9));
        assert_eq!(battery_action(&BatteryStatus { percent: 10, status: "Discharging".into() }), BatteryAction::None);
    }
}
