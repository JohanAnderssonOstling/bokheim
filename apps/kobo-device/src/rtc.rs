//! Linux RTC wake alarm used for automatic power-off during suspend.
use libc::{c_int, c_ulong, gmtime_r, ioctl, time_t, tm};
use std::cell::Cell;
use std::fs::{self, File};
use std::io::{self, Write};
use std::mem::{MaybeUninit, size_of};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RtcTime {
    sec: c_int,
    min: c_int,
    hour: c_int,
    day: c_int,
    month: c_int,
    year: c_int,
    weekday: c_int,
    yearday: c_int,
    dst: c_int,
}

impl RtcTime {
    fn key(self) -> (c_int, c_int, c_int, c_int, c_int, c_int) {
        (self.year, self.month, self.day, self.hour, self.min, self.sec)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WakeAlarm {
    enabled: u8,
    pending: u8,
    time: RtcTime,
}

impl WakeAlarm {
    fn encode(self) -> String {
        let time = self.time;
        format!("{} {} {} {} {} {} {} {} {} {} {}\n", self.enabled, self.pending, time.sec, time.min, time.hour, time.day, time.month, time.year, time.weekday, time.yearday, time.dst)
    }

    fn decode(line: &str) -> io::Result<Self> {
        let values = line.split_whitespace().map(str::parse::<c_int>).collect::<Result<Vec<_>, _>>().map_err(io::Error::other)?;
        let [enabled, pending, sec, min, hour, day, month, year, weekday, yearday, dst] = values.as_slice() else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid saved RTC alarm"));
        };
        if !(0..=1).contains(enabled) || !(0..=1).contains(pending) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid saved RTC alarm flags"));
        }
        Ok(Self { enabled: *enabled as u8, pending: *pending as u8, time: RtcTime { sec: *sec, min: *min, hour: *hour, day: *day, month: *month, year: *year, weekday: *weekday, yearday: *yearday, dst: *dst } })
    }
}

const SET_ALARM: c_ulong = (1 << 30) | ((size_of::<WakeAlarm>() as c_ulong) << 16) | (b'p' as c_ulong) << 8 | 0x0f;
const READ_ALARM: c_ulong = (2 << 30) | ((size_of::<WakeAlarm>() as c_ulong) << 16) | (b'p' as c_ulong) << 8 | 0x10;
const DISABLE_ALARM: c_ulong = (b'p' as c_ulong) << 8 | 0x02;

fn open_device() -> io::Result<File> {
    File::open("/dev/rtc0").or_else(|_| File::open("/dev/rtc"))
}

fn read_alarm(device: &File) -> io::Result<WakeAlarm> {
    let mut alarm = WakeAlarm::default();
    if unsafe { ioctl(device.as_raw_fd(), READ_ALARM, &mut alarm) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(alarm)
}

fn set_alarm(device: &File, alarm: &WakeAlarm) -> io::Result<()> {
    if unsafe { ioctl(device.as_raw_fd(), SET_ALARM, alarm) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn journal_path() -> Option<PathBuf> {
    std::env::var_os("BOKHEIM_KOBO_RTC_STATE_FILE").map(PathBuf::from)
}

fn save_alarm(path: &Path, previous: WakeAlarm, owned: WakeAlarm) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| io::Error::other("RTC recovery directory is missing"))?;
    let mut staged = tempfile::NamedTempFile::new_in(directory)?;
    write!(staged, "bokheim-rtc-v1\n{}{}", previous.encode(), owned.encode())?;
    staged.as_file().sync_all()?;
    staged.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}

fn restore_alarm(device: &File, previous: WakeAlarm, owned: WakeAlarm) -> io::Result<()> {
    let current = read_alarm(device)?;
    // An alarm changed by another owner must not be overwritten on cleanup.
    if current.time.key() != owned.time.key() && current.time.key() != previous.time.key() {
        return Ok(());
    }
    let previous_future = previous.time.key() > rtc_time(SystemTime::now())?.key();
    if current.time.key() == previous.time.key() && current.enabled == previous.enabled && (previous.enabled == 0 || previous_future) {
        return Ok(());
    }
    if previous.enabled != 0 && previous_future {
        set_alarm(device, &WakeAlarm { pending: 0, ..previous })
    } else if unsafe { ioctl(device.as_raw_fd(), DISABLE_ALARM) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// The launcher calls this after the app exits, including after a crash/signal.
pub(super) fn restore_pending() -> io::Result<()> {
    let Some(path) = journal_path() else { return Ok(()) };
    let saved = match fs::read_to_string(&path) {
        Ok(saved) => saved,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let lines: Vec<_> = saved.lines().collect();
    let ["bokheim-rtc-v1", previous, owned] = lines.as_slice() else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid RTC recovery journal"));
    };
    restore_alarm(&open_device()?, WakeAlarm::decode(previous)?, WakeAlarm::decode(owned)?)?;
    fs::remove_file(path)
}

fn rtc_time(deadline: SystemTime) -> io::Result<RtcTime> {
    let seconds: time_t = deadline.duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_secs().try_into().map_err(io::Error::other)?;
    let mut time = MaybeUninit::<tm>::uninit();
    if unsafe { gmtime_r(&seconds, time.as_mut_ptr()) }.is_null() {
        return Err(io::Error::last_os_error());
    }
    let time = unsafe { time.assume_init() };
    Ok(RtcTime { sec: time.tm_sec, min: time.tm_min, hour: time.tm_hour, day: time.tm_mday, month: time.tm_mon, year: time.tm_year, weekday: -1, yearday: -1, dst: -1 })
}

pub(super) struct Alarm {
    device: File,
    deadline: SystemTime,
    previous: WakeAlarm,
    owned: WakeAlarm,
    using_previous: Cell<bool>,
    journal: Option<PathBuf>,
}
impl Alarm {
    pub fn arm(duration: Duration) -> io::Result<Self> {
        let deadline = SystemTime::now().checked_add(duration).ok_or_else(|| io::Error::other("RTC deadline overflow"))?;
        restore_pending()?;
        let device = open_device()?;
        let previous = read_alarm(&device)?;
        let owned = WakeAlarm { enabled: 1, pending: 0, time: rtc_time(deadline)? };
        let using_previous = previous.enabled != 0 && previous.time.key() > rtc_time(SystemTime::now())?.key() && previous.time.key() < owned.time.key();
        let journal = journal_path();
        if let Some(path) = &journal {
            save_alarm(path, previous, owned)?;
        }
        let alarm = Self { device, deadline, previous, owned, using_previous: Cell::new(using_previous), journal };
        // Keep an earlier firmware wake intact. After it fires, due() arms
        // Bokheim's later shutdown deadline for the next suspend cycle.
        if !using_previous {
            set_alarm(&alarm.device, &owned)?;
        }
        Ok(alarm)
    }
    pub fn due(&self) -> bool {
        let now = SystemTime::now();
        if now >= self.deadline {
            return true;
        }
        let Ok(current) = read_alarm(&self.device) else { return false };
        if self.using_previous.get() {
            if current.time.key() == self.previous.time.key()
                && (current.pending != 0 || rtc_time(now).is_ok_and(|now| now.key() >= self.previous.time.key()))
            {
                match set_alarm(&self.device, &self.owned) {
                    Ok(()) => self.using_previous.set(false),
                    Err(error) => eprintln!("Could not arm Kobo shutdown after firmware wake: {error}"),
                }
            }
            return false;
        }
        current.time.key() == self.owned.time.key() && current.pending != 0
    }
}
impl Drop for Alarm {
    fn drop(&mut self) {
        if let Err(error) = restore_alarm(&self.device, self.previous, self.owned) {
            // Keep the journal so the launcher can retry after the app exits.
            eprintln!("Could not restore Kobo RTC alarm: {error}");
        } else if let Some(path) = &self.journal
            && let Err(error) = fs::remove_file(path)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("Could not remove Kobo RTC recovery journal: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rtc_abi_and_utc_calendar_match_linux() {
        assert_eq!(size_of::<WakeAlarm>(), 40);
        assert_eq!((SET_ALARM, READ_ALARM, DISABLE_ALARM), (0x4028700f, 0x80287010, 0x7002));
        let date = rtc_time(UNIX_EPOCH + Duration::from_secs(86_400)).unwrap();
        assert_eq!((date.year, date.month, date.day, date.hour, date.min, date.sec), (70, 0, 2, 0, 0, 0));
    }
}
