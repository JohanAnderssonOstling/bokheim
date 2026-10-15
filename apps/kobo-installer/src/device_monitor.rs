//! Mount recognized Kobo USB volumes without blocking the GUI or prompting for privileges.
use crate::{KoboDevice, detect_kobo_devices};

#[derive(Default)]
pub struct DeviceMonitor {
    #[cfg(target_os = "linux")]
    attempts: std::collections::BTreeMap<String, Option<String>>,
}

pub struct DeviceDetection {
    pub devices: Vec<KoboDevice>,
    pub mount_error: Option<String>,
}

impl DeviceMonitor {
    pub fn poll(&mut self) -> DeviceDetection {
        #[cfg(target_os = "linux")]
        let mount_error = self.mount_linux();
        #[cfg(not(target_os = "linux"))]
        let mount_error = None;
        DeviceDetection { devices: detect_kobo_devices(), mount_error }
    }

    #[cfg(target_os = "linux")]
    fn mount_linux(&mut self) -> Option<String> {
        use std::process::Command;
        let output = match Command::new("lsblk").args(["--json", "--paths", "--tree", "--output", "PATH,TYPE,FSTYPE,LABEL,MOUNTPOINTS,TRAN,RM"]).output() {
            Ok(output) if output.status.success() => output,
            _ => return None, // Mounted-volume detection still works without lsblk.
        };
        let snapshot: BlockSnapshot = match serde_json::from_slice(&output.stdout) {
            Ok(snapshot) => snapshot,
            Err(_) => return None,
        };
        let mut candidates = Vec::new();
        for device in &snapshot.blockdevices {
            collect_kobos(device, false, false, &mut candidates);
        }
        self.process_candidates(&candidates, |path| {
            let output = Command::new("udisksctl").args(["mount", "--block-device", path, "--no-user-interaction"]).output().map_err(|error| error.to_string())?;
            if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
        })
    }

    #[cfg(target_os = "linux")]
    fn process_candidates(&mut self, candidates: &[MountCandidate], mut mount: impl FnMut(&str) -> Result<(), String>) -> Option<String> {
        // Remember mounted volumes too, so a safe eject is never undone by this monitor.
        // Removal clears the record; reconnecting enables one new mount attempt.
        self.attempts.retain(|key, _| candidates.iter().any(|candidate| &candidate.key == key));
        for candidate in candidates {
            if candidate.mounted {
                self.attempts.insert(candidate.key.clone(), None);
            } else if !self.attempts.contains_key(&candidate.key) {
                self.attempts.insert(candidate.key.clone(), mount(&candidate.path).err());
            }
        }
        self.attempts.values().find_map(Clone::clone)
    }
}

#[cfg(target_os = "linux")]
#[derive(serde::Deserialize)]
struct BlockSnapshot {
    blockdevices: Vec<BlockDevice>,
}

#[cfg(target_os = "linux")]
#[derive(serde::Deserialize)]
struct BlockDevice {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    fstype: Option<String>,
    label: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    tran: Option<String>,
    rm: bool,
    #[serde(default)]
    children: Vec<BlockDevice>,
}

#[cfg(target_os = "linux")]
struct MountCandidate {
    path: String,
    key: String,
    mounted: bool,
}

#[cfg(target_os = "linux")]
fn collect_kobos(device: &BlockDevice, parent_usb: bool, parent_removable: bool, result: &mut Vec<MountCandidate>) {
    use std::{fs, path::Path};
    let usb = device.tran.as_deref() == Some("usb") || parent_usb;
    let removable = device.rm || parent_removable;
    if usb && removable && matches!(device.kind.as_str(), "disk" | "part") && device.label.as_deref() == Some("KOBOeReader") && device.fstype.as_deref() == Some("vfat") && device.path.starts_with("/dev/") {
        let name = Path::new(&device.path).file_name().unwrap_or_default().to_string_lossy();
        // Linux disk sequence changes even if a fast reconnect reuses the same /dev path.
        let sysfs = Path::new("/sys/class/block").join(name.as_ref());
        let sequence = fs::read_to_string(sysfs.join("diskseq"))
            .or_else(|_| {
                let resolved = sysfs.canonicalize()?;
                fs::read_to_string(resolved.parent().unwrap_or(&resolved).join("diskseq"))
            })
            .unwrap_or_default();
        result.push(MountCandidate { key: format!("{}:{}", device.path, sequence.trim()), path: device.path.clone(), mounted: device.mountpoints.iter().any(|point| point.as_ref().is_some_and(|point| !point.is_empty())) });
    }
    for child in &device.children {
        collect_kobos(child, usb, removable, result);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn candidate(key: &str, mounted: bool) -> MountCandidate {
        MountCandidate { path: "/dev/fakekobo".into(), key: key.into(), mounted }
    }

    #[test]
    fn mounts_once_per_connection_and_does_not_undo_eject() {
        let mut monitor = DeviceMonitor::default();
        let mut attempts = 0;
        monitor.process_candidates(&[candidate("1", false)], |_| {
            attempts += 1;
            Ok(())
        });
        monitor.process_candidates(&[candidate("1", true)], |_| panic!("already mounted"));
        monitor.process_candidates(&[candidate("1", false)], |_| panic!("must not undo eject"));
        monitor.process_candidates(&[], |_| panic!("disconnected"));
        monitor.process_candidates(&[candidate("2", false)], |_| {
            attempts += 1;
            Ok(())
        });
        assert_eq!(attempts, 2);
    }

    #[test]
    fn permission_denial_is_reported_without_repeated_attempts() {
        let mut monitor = DeviceMonitor::default();
        let error = monitor.process_candidates(&[candidate("1", false)], |_| Err("Not authorized".into()));
        assert_eq!(error.as_deref(), Some("Not authorized"));
        assert_eq!(monitor.process_candidates(&[candidate("1", false)], |_| panic!("no retry loop")), error);
        assert_eq!(monitor.process_candidates(&[candidate("1", true)], |_| panic!("manually mounted")), None);
    }

    #[test]
    fn recognizes_only_removable_usb_kobo_fat_volumes_including_partitions() {
        let snapshot: BlockSnapshot = serde_json::from_str(
            r#"{"blockdevices":[
          {"path":"/dev/fakekobo","type":"disk","fstype":"vfat","label":"KOBOeReader","tran":"usb","rm":true,"mountpoints":[null]},
          {"path":"/dev/fakeparent","type":"disk","tran":"usb","rm":true,"children":[
            {"path":"/dev/fakepartition","type":"part","fstype":"vfat","label":"KOBOeReader","rm":false,"mountpoints":["/media/Kobo"]}]},
          {"path":"/dev/fakesystem","type":"disk","fstype":"vfat","label":"KOBOeReader","tran":"sata","rm":false},
          {"path":"/dev/fakeotherusb","type":"disk","fstype":"vfat","label":"OTHER","tran":"usb","rm":true}
        ]}"#,
        )
        .unwrap();
        let mut candidates = vec![];
        for device in &snapshot.blockdevices {
            collect_kobos(device, false, false, &mut candidates);
        }
        assert_eq!(candidates.iter().map(|device| device.path.as_str()).collect::<Vec<_>>(), ["/dev/fakekobo", "/dev/fakepartition"]);
        assert!(!candidates[0].mounted);
        assert!(candidates[1].mounted);
    }
}
