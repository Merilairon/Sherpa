//! Device drivers and discovery.

pub mod displaypad;
pub mod everest;
pub mod everest60;
pub mod makalu;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use basecamp_core::types::DeviceKind;
use basecamp_core::VENDOR_ID;
use basecamp_hid::{DeviceInfo, HidIo, ReportKind};

use crate::link::Endpoint;
use crate::store::device_key;

/// A newly discovered, opened device.
pub struct Found {
    pub key: String,
    pub identity: String,
    pub kind: DeviceKind,
    pub pid: u16,
    pub product: String,
    pub main: Endpoint,
    /// DisplayPad bulk image channel.
    pub bulk: Option<Endpoint>,
}

/// Source of devices: real HID or simulated.
pub trait Backend: Send + Sync {
    /// Returns the keys of all present devices and the opened endpoints of devices that
    /// are not in `attached`.
    fn scan(&self, attached: &HashSet<String>) -> (HashSet<String>, Vec<Found>);
}

impl<T: Backend> Backend for Arc<T> {
    fn scan(&self, attached: &HashSet<String>) -> (HashSet<String>, Vec<Found>) {
        (**self).scan(attached)
    }
}

/// Real HID devices.
pub struct HidBackend;

fn endpoint(io: Arc<dyn HidIo>, info: &DeviceInfo) -> Endpoint {
    let in_ids = info.collection.as_ref().map(|c| c.report_ids.clone()).unwrap_or_default();
    Endpoint::new(io, info.report_id(ReportKind::Output), in_ids)
}

fn is_vendor(i: &DeviceInfo, page: u16, usage: u16) -> bool {
    i.usage_page == page && i.usage == usage
}

/// Pick the collections a device kind needs (spec 04 §1, 09 §1, 10 §1, 11 §1).
fn pick(kind: DeviceKind, infos: &[&DeviceInfo]) -> Option<(DeviceInfo, Option<DeviceInfo>)> {
    let find = |f: &dyn Fn(&DeviceInfo) -> bool| infos.iter().find(|i| f(i)).map(|i| (*i).clone());
    match kind {
        DeviceKind::EverestMax | DeviceKind::MacroPad => Some((find(&|i| is_vendor(i, 0xFF00, 0x01))?, None)),
        DeviceKind::DisplayPad => Some((find(&|i| is_vendor(i, 0xFF00, 0x01))?, find(&|i| is_vendor(i, 0xFF01, 0x02)))),
        DeviceKind::Everest60 => {
            let main = find(&|i| i.interface == 2 && i.usage_page >= 0xFF00)
                .or_else(|| find(&|i| i.interface == 2))
                .or_else(|| find(&|i| i.usage_page >= 0xFF00))
                .or_else(|| infos.first().map(|i| (*i).clone()))?;
            Some((main, None))
        }
        DeviceKind::Makalu67 => {
            let has_a1 = |i: &DeviceInfo| i.collection.as_ref().is_some_and(|c| c.has_report(ReportKind::Feature, 0xA1));
            let main = find(&|i| i.interface == 0 && has_a1(i))
                .or_else(|| find(&|i| has_a1(i)))
                .or_else(|| find(&|i| i.interface == 0 && i.usage_page >= 0xFF00))
                .or_else(|| find(&|i| i.interface == 0))?;
            Some((main, None))
        }
    }
}

impl Backend for HidBackend {
    fn scan(&self, attached: &HashSet<String>) -> (HashSet<String>, Vec<Found>) {
        let all = basecamp_hid::enumerate(VENDOR_ID);
        // Group collections by physical device.
        let mut groups: BTreeMap<(DeviceKind, String), Vec<&DeviceInfo>> = BTreeMap::new();
        for i in &all {
            if let Some(kind) = DeviceKind::from_pid(i.pid) {
                let ident = if kind.multi_instance() { i.identity.clone() } else { String::new() };
                groups.entry((kind, ident)).or_default().push(i);
            }
        }
        let mut present = HashSet::new();
        let mut found = Vec::new();
        for ((kind, ident), infos) in groups {
            let key = device_key(kind, &ident);
            present.insert(key.clone());
            if attached.contains(&key) {
                continue;
            }
            let Some((main, bulk)) = pick(kind, &infos) else {
                log::warn!("{}: no usable HID collection", kind.name());
                continue;
            };
            // Linux exposes several collections on one node: open each path once.
            let mut opened: HashMap<String, Arc<dyn HidIo>> = HashMap::new();
            let mut open = |path: &str| -> Option<Arc<dyn HidIo>> {
                if let Some(io) = opened.get(path) {
                    return Some(io.clone());
                }
                match basecamp_hid::open(path) {
                    Ok(io) => {
                        opened.insert(path.to_string(), io.clone());
                        Some(io)
                    }
                    Err(e) => {
                        log::warn!("cannot open {path}: {e} (check the udev rule / permissions)");
                        None
                    }
                }
            };
            let Some(main_io) = open(&main.path) else { continue };
            let bulk_ep = match &bulk {
                Some(b) => match open(&b.path) {
                    Some(io) => Some(endpoint(io, b)),
                    None => continue,
                },
                None if kind == DeviceKind::DisplayPad => {
                    log::warn!("DisplayPad: bulk image collection FF01:02 not found");
                    None
                }
                None => None,
            };
            found.push(Found {
                key,
                identity: if ident.is_empty() { main.identity.clone() } else { ident },
                kind,
                pid: main.pid,
                product: main.product.clone(),
                main: endpoint(main_io, &main),
                bulk: bulk_ep,
            });
        }
        (present, found)
    }
}

/// Simulated devices for `--fake` and tests.
pub struct FakeBackend {
    devices: Mutex<Vec<(String, Option<Found>)>>,
    present: Mutex<HashSet<String>>,
}

impl FakeBackend {
    pub fn new(found: Vec<Found>) -> Self {
        let present = found.iter().map(|f| f.key.clone()).collect();
        FakeBackend { devices: Mutex::new(found.into_iter().map(|f| (f.key.clone(), Some(f))).collect()), present: Mutex::new(present) }
    }

    /// Add a device that will be reported on the next scan.
    pub fn plug(&self, f: Found) {
        self.present.lock().unwrap().insert(f.key.clone());
        self.devices.lock().unwrap().push((f.key.clone(), Some(f)));
    }

    pub fn unplug(&self, key: &str) {
        self.present.lock().unwrap().remove(key);
    }

    /// One of each supported device.
    pub fn demo() -> (Self, crate::fake::Sims) {
        let (found, sims) = crate::fake::demo_devices();
        (FakeBackend::new(found), sims)
    }
}

impl Backend for FakeBackend {
    fn scan(&self, attached: &HashSet<String>) -> (HashSet<String>, Vec<Found>) {
        let present = self.present.lock().unwrap().clone();
        let mut out = Vec::new();
        for (key, f) in self.devices.lock().unwrap().iter_mut() {
            if present.contains(key) && !attached.contains(key) {
                if let Some(f) = f.take() {
                    out.push(f);
                }
            }
        }
        (present, out)
    }
}
