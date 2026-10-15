//! Linux RTC wake alarm used for automatic power-off during suspend.
use libc::{c_int, c_ulong, gmtime_r, ioctl, time_t, tm};
use std::fs::File;
use std::io;
use std::mem::{MaybeUninit, size_of};
use std::os::fd::AsRawFd;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[repr(C)]
#[derive(Default)]
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

#[repr(C)]
#[derive(Default)]
struct WakeAlarm {
    enabled: u8,
    pending: u8,
    time: RtcTime,
}

const SET_ALARM: c_ulong = (1 << 30) | ((size_of::<WakeAlarm>() as c_ulong) << 16) | (b'p' as c_ulong) << 8 | 0x0f;
const READ_ALARM: c_ulong = (2 << 30) | ((size_of::<WakeAlarm>() as c_ulong) << 16) | (b'p' as c_ulong) << 8 | 0x10;
const DISABLE_ALARM: c_ulong = (b'p' as c_ulong) << 8 | 0x02;

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
}
impl Alarm {
    pub fn arm(duration: Duration) -> io::Result<Self> {
        let deadline = SystemTime::now().checked_add(duration).ok_or_else(|| io::Error::other("RTC deadline overflow"))?;
        let device = File::open("/dev/rtc0").or_else(|_| File::open("/dev/rtc"))?;
        let alarm = WakeAlarm { enabled: 1, pending: 0, time: rtc_time(deadline)? };
        if unsafe { ioctl(device.as_raw_fd(), SET_ALARM, &alarm) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { device, deadline })
    }
    pub fn due(&self) -> bool {
        let mut alarm = WakeAlarm::default();
        let pending = unsafe { ioctl(self.device.as_raw_fd(), READ_ALARM, &mut alarm) } >= 0 && alarm.pending != 0;
        pending || SystemTime::now() >= self.deadline
    }
}
impl Drop for Alarm {
    fn drop(&mut self) {
        if unsafe { ioctl(self.device.as_raw_fd(), DISABLE_ALARM) } < 0 {
            eprintln!("Could not clear Kobo RTC alarm: {}", io::Error::last_os_error());
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
