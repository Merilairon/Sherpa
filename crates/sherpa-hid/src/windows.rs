//! Windows backend on top of the native HID API (via `hidapi`). Each top-level collection
//! is its own device path. Two handles are opened so a blocking read never delays a write.

use std::ffi::CString;
use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::descriptor;
use crate::{DeviceInfo, HidIo};

fn api() -> &'static Mutex<Option<HidApi>> {
    static API: OnceLock<Mutex<Option<HidApi>>> = OnceLock::new();
    API.get_or_init(|| Mutex::new(None))
}

fn new_api() -> hidapi::HidResult<HidApi> {
    HidApi::disable_device_discovery();
    HidApi::new()
}

fn err(e: hidapi::HidError) -> io::Error {
    io::Error::other(e.to_string())
}

struct WinHid {
    reader: Mutex<HidDevice>,
    writer: Mutex<HidDevice>,
}

impl HidIo for WinHid {
    fn write(&self, data: &[u8]) -> io::Result<()> {
        self.writer.lock().unwrap().write(data).map(|_| ()).map_err(err)
    }

    fn read(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; 4096];
        let n = self.reader.lock().unwrap().read_timeout(&mut buf, timeout.as_millis() as i32).map_err(err)?;
        if n == 0 {
            return Ok(None);
        }
        buf.truncate(n);
        Ok(Some(buf))
    }

    fn set_feature(&self, data: &[u8]) -> io::Result<()> {
        self.writer.lock().unwrap().send_feature_report(data).map_err(err)
    }

    fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        buf[0] = report_id;
        let n = self.writer.lock().unwrap().get_feature_report(&mut buf).map_err(err)?;
        buf.truncate(n);
        Ok(buf)
    }
}

fn open_dev(api: &HidApi, path: &str) -> io::Result<HidDevice> {
    let c = CString::new(path).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad path"))?;
    api.open_path(&c).map_err(err)
}

pub fn open(path: &str) -> io::Result<Arc<dyn HidIo>> {
    let mut guard = api().lock().unwrap();
    if guard.is_none() {
        *guard = Some(new_api().map_err(err)?);
    }
    let api = guard.as_ref().unwrap();
    let reader = open_dev(api, path)?;
    let writer = open_dev(api, path)?;
    Ok(Arc::new(WinHid { reader: Mutex::new(reader), writer: Mutex::new(writer) }))
}

/// A path without the collection suffix identifies the interface instance.
fn identity_of(path: &str, serial: &str) -> String {
    if !serial.is_empty() {
        return serial.to_string();
    }
    let lower = path.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("&col") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 4..];
        let skip = tail.chars().take_while(|c| c.is_ascii_hexdigit()).count();
        rest = &tail[skip..];
    }
    out.push_str(rest);
    out
}

pub fn enumerate(vid: u16) -> Vec<DeviceInfo> {
    let mut guard = api().lock().unwrap();
    if guard.is_none() {
        match new_api() {
            Ok(a) => *guard = Some(a),
            Err(e) => {
                log::warn!("HID API unavailable: {e}");
                return Vec::new();
            }
        }
    }
    let api = guard.as_mut().unwrap();
    if let Err(e) = api.reset_devices().and_then(|_| api.add_devices(vid, 0)) {
        log::warn!("HID enumeration failed: {e}");
        return Vec::new();
    }
    let mut out = Vec::new();
    for d in api.device_list() {
        let path = d.path().to_string_lossy().to_string();
        let serial = d.serial_number().unwrap_or("").to_string();
        // Read the descriptor to learn the report ids of this collection.
        let collection = api.open_path(d.path()).ok().and_then(|dev| {
            let mut buf = vec![0u8; 4096];
            let n = dev.get_report_descriptor(&mut buf).ok()?;
            descriptor::parse(&buf[..n]).into_iter().find(|c| c.usage_page == d.usage_page() && c.usage == d.usage())
        });
        out.push(DeviceInfo {
            identity: identity_of(&path, &serial),
            path,
            vid: d.vendor_id(),
            pid: d.product_id(),
            interface: d.interface_number(),
            usage_page: d.usage_page(),
            usage: d.usage(),
            collection,
            serial,
            product: d.product_string().unwrap_or("").to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity() {
        let p = r"\\?\HID#VID_3282&PID_0009&MI_01&Col02#8&2a1b&0&0001#{4d1e55b2}";
        let q = r"\\?\HID#VID_3282&PID_0009&MI_01&Col01#8&2a1b&0&0000#{4d1e55b2}";
        assert_eq!(super::identity_of(p, "").replace("0001", "0000"), super::identity_of(q, ""));
    }
}
