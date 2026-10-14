//! HID transport. Linux talks to `/dev/hidraw*` directly (no libudev or libusb needed);
//! Windows uses the native HID API through `hidapi`.

pub mod descriptor;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

use std::io;
use std::sync::Arc;
use std::time::Duration;

pub use descriptor::{Collection, ReportKind};

/// An open HID device node. All methods take `&self` and may be called from several
/// threads (one reader, one writer).
pub trait HidIo: Send + Sync {
    /// Write an output report. `data[0]` is the report id (0 when unused).
    fn write(&self, data: &[u8]) -> io::Result<()>;
    /// Read one input report. `Ok(None)` on timeout. When the device uses report ids
    /// the first byte is the id.
    fn read(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
    /// HID SetFeature. `data[0]` is the report id.
    fn set_feature(&self, data: &[u8]) -> io::Result<()>;
    /// HID GetFeature of `len` bytes including the report id byte at index 0.
    fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>>;
}

/// One top-level collection of an attached device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Path to pass to [`open`].
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    /// USB interface number, -1 if unknown.
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
    /// Parsed collection (report ids and sizes); `None` when the descriptor is unavailable.
    pub collection: Option<Collection>,
    pub serial: String,
    pub product: String,
    /// Stable identity of the physical device (same for all its collections when the
    /// platform allows it).
    pub identity: String,
}

impl DeviceInfo {
    pub fn report_id(&self, kind: ReportKind) -> u8 {
        self.collection.as_ref().map(|c| c.report_id(kind)).unwrap_or(0)
    }

    pub fn uses_report_ids(&self) -> bool {
        self.collection.as_ref().map(|c| c.uses_report_ids()).unwrap_or(false)
    }
}

/// List the HID collections of all attached devices with the given vendor id.
pub fn enumerate(vid: u16) -> Vec<DeviceInfo> {
    #[cfg(target_os = "linux")]
    {
        linux::enumerate(vid)
    }
    #[cfg(windows)]
    {
        windows::enumerate(vid)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = vid;
        Vec::new()
    }
}

/// Open a device path returned by [`enumerate`].
pub fn open(path: &str) -> io::Result<Arc<dyn HidIo>> {
    #[cfg(target_os = "linux")]
    {
        linux::open(path)
    }
    #[cfg(windows)]
    {
        windows::open(path)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = path;
        Err(io::Error::new(io::ErrorKind::Unsupported, "HID is not supported on this platform"))
    }
}
