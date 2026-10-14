//! Linux hidraw backend.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::descriptor;
use crate::{DeviceInfo, HidIo};

struct Hidraw {
    file: File,
}

// _IOC(dir, type, nr, size) for the generic ioctl layout (x86, arm, riscv).
const fn ioc(dir: u64, ty: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | (ty << 8) | nr
}
const IOC_RW: u64 = 3;

fn hidiocsfeature(len: usize) -> u64 {
    ioc(IOC_RW, b'H' as u64, 0x06, len as u64)
}
fn hidiocgfeature(len: usize) -> u64 {
    ioc(IOC_RW, b'H' as u64, 0x07, len as u64)
}

impl HidIo for Hidraw {
    fn write(&self, data: &[u8]) -> io::Result<()> {
        let n = (&self.file).write(data)?;
        if n != data.len() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short HID write"));
        }
        Ok(())
    }

    fn read(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let r = unsafe { libc::poll(&mut pfd, 1, ms) };
        if r < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(e);
        }
        if r == 0 {
            return Ok(None);
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "device disconnected"));
        }
        let mut buf = vec![0u8; 4096];
        match (&self.file).read(&mut buf) {
            Ok(0) => Err(io::Error::new(io::ErrorKind::BrokenPipe, "device disconnected")),
            Ok(n) => {
                buf.truncate(n);
                Ok(Some(buf))
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn set_feature(&self, data: &[u8]) -> io::Result<()> {
        let mut buf = data.to_vec();
        let r = unsafe { libc::ioctl(self.file.as_raw_fd(), hidiocsfeature(buf.len()) as _, buf.as_mut_ptr()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        buf[0] = report_id;
        let r = unsafe { libc::ioctl(self.file.as_raw_fd(), hidiocgfeature(len) as _, buf.as_mut_ptr()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(r as usize);
        Ok(buf)
    }
}

pub fn open(path: &str) -> io::Result<Arc<dyn HidIo>> {
    let file = OpenOptions::new().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(path)?;
    Ok(Arc::new(Hidraw { file }))
}

fn uevent(dir: &Path) -> Vec<(String, String)> {
    fs::read_to_string(dir.join("device/uevent"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect()
}

fn interface_number(dir: &Path, phys: &str) -> i32 {
    // device -> HID device; its parent is the USB interface with bInterfaceNumber.
    if let Ok(real) = fs::canonicalize(dir.join("device")) {
        if let Some(parent) = real.parent() {
            if let Ok(s) = fs::read_to_string(parent.join("bInterfaceNumber")) {
                if let Ok(n) = i32::from_str_radix(s.trim(), 16) {
                    return n;
                }
            }
        }
    }
    phys.rsplit_once("/input").and_then(|(_, n)| n.parse().ok()).unwrap_or(-1)
}

pub fn enumerate(vid: u16) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else { return out };
    let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let ev = uevent(&dir);
        let get = |k: &str| ev.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone()).unwrap_or_default();
        // HID_ID=0003:00003282:00000001
        let id = get("HID_ID");
        let parts: Vec<&str> = id.split(':').collect();
        if parts.len() != 3 {
            continue;
        }
        let (Ok(v), Ok(p)) = (u32::from_str_radix(parts[1], 16), u32::from_str_radix(parts[2], 16)) else { continue };
        if v as u16 != vid {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let path = format!("/dev/{name}");
        let phys = get("HID_PHYS");
        let serial = get("HID_UNIQ");
        let identity = {
            let base = phys.rsplit_once("/input").map(|(b, _)| b.to_string()).unwrap_or(phys.clone());
            if serial.is_empty() {
                base
            } else {
                format!("{base}#{serial}")
            }
        };
        let interface = interface_number(&dir, &phys);
        let desc = fs::read(dir.join("device/report_descriptor")).unwrap_or_default();
        let collections = descriptor::parse(&desc);
        if collections.is_empty() {
            out.push(DeviceInfo {
                path,
                vid: v as u16,
                pid: p as u16,
                interface,
                usage_page: 0,
                usage: 0,
                collection: None,
                serial,
                product: get("HID_NAME"),
                identity,
            });
            continue;
        }
        for c in collections {
            out.push(DeviceInfo {
                path: path.clone(),
                vid: v as u16,
                pid: p as u16,
                interface,
                usage_page: c.usage_page,
                usage: c.usage,
                collection: Some(c),
                serial: serial.clone(),
                product: get("HID_NAME"),
                identity: identity.clone(),
            });
        }
    }
    out
}
