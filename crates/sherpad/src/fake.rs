//! Simulated devices. Used by the test-suite (spec 07 §3 "fake device") and by
//! `sherpad --fake`, which lets the UI be explored without hardware.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use sherpa_core::everest::{ExtendInfo, Variant};
use sherpa_core::{packet, tables, Packet};
use sherpa_hid::HidIo;

/// One operation seen on the fake wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireOp {
    Write(Vec<u8>),
    SetFeature(Vec<u8>),
    GetFeature(u8),
}

type Handler = Box<dyn FnMut(&WireOp) -> Vec<Vec<u8>> + Send>;

/// A scriptable HID node.
pub struct FakeHid {
    log: Mutex<Vec<WireOp>>,
    input: Mutex<VecDeque<Vec<u8>>>,
    cv: Condvar,
    feature: Mutex<Option<Vec<u8>>>,
    handler: Mutex<Handler>,
    gone: AtomicBool,
}

impl FakeHid {
    pub fn new(handler: impl FnMut(&WireOp) -> Vec<Vec<u8>> + Send + 'static) -> Arc<FakeHid> {
        Arc::new(FakeHid {
            log: Mutex::new(Vec::new()),
            input: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
            feature: Mutex::new(None),
            handler: Mutex::new(Box::new(handler)),
            gone: AtomicBool::new(false),
        })
    }

    /// An endpoint that acknowledges every write by echoing it.
    pub fn echo() -> Arc<FakeHid> {
        FakeHid::new(|op| match op {
            WireOp::Write(d) => vec![d[1..].to_vec()],
            _ => vec![],
        })
    }

    pub fn set_handler(&self, h: impl FnMut(&WireOp) -> Vec<Vec<u8>> + Send + 'static) {
        *self.handler.lock().unwrap() = Box::new(h);
    }

    /// Queue an input report (as read from the node, i.e. without report id here since
    /// fake endpoints declare no ids).
    pub fn inject(&self, r: Vec<u8>) {
        self.input.lock().unwrap().push_back(r);
        self.cv.notify_all();
    }

    pub fn log(&self) -> Vec<WireOp> {
        self.log.lock().unwrap().clone()
    }

    /// Written output payloads (report id stripped).
    pub fn writes(&self) -> Vec<Vec<u8>> {
        self.log()
            .into_iter()
            .filter_map(|o| match o {
                WireOp::Write(d) => Some(d[1..].to_vec()),
                _ => None,
            })
            .collect()
    }

    pub fn clear_log(&self) {
        self.log.lock().unwrap().clear();
    }

    pub fn unplug(&self) {
        self.gone.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }

    fn run(&self, op: WireOp) -> Vec<Vec<u8>> {
        self.log.lock().unwrap().push(op.clone());
        (self.handler.lock().unwrap())(&op)
    }
}

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "unplugged")
}

impl HidIo for FakeHid {
    fn write(&self, data: &[u8]) -> io::Result<()> {
        if self.gone.load(Ordering::SeqCst) {
            return Err(gone());
        }
        for r in self.run(WireOp::Write(data.to_vec())) {
            self.inject(r);
        }
        Ok(())
    }

    fn read(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let deadline = Instant::now() + timeout;
        let mut q = self.input.lock().unwrap();
        loop {
            if self.gone.load(Ordering::SeqCst) {
                return Err(gone());
            }
            if let Some(r) = q.pop_front() {
                return Ok(Some(r));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            q = self.cv.wait_timeout(q, deadline - now).unwrap().0;
        }
    }

    fn set_feature(&self, data: &[u8]) -> io::Result<()> {
        if self.gone.load(Ordering::SeqCst) {
            return Err(gone());
        }
        let r = self.run(WireOp::SetFeature(data.to_vec()));
        *self.feature.lock().unwrap() = r.into_iter().next();
        Ok(())
    }

    fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        if self.gone.load(Ordering::SeqCst) {
            return Err(gone());
        }
        let r = self.run(WireOp::GetFeature(report_id)).into_iter().next();
        let mut v = r.or_else(|| self.feature.lock().unwrap().clone()).unwrap_or_else(|| vec![report_id]);
        v.resize(len, 0);
        Ok(v)
    }
}

// ---------------------------------------------------------------------------------------
// Everest Max / MacroPad simulator

/// Predicate over a written payload.
pub type PacketFilter = Box<dyn Fn(&[u8]) -> bool + Send>;

/// Mutable state of a simulated Everest-family device.
pub struct EverestSim {
    pub variant: Variant,
    pub profile: u8,
    pub menu: u8,
    pub ext: ExtendInfo,
    pub clock_digital: bool,
    pub clock_12h: bool,
    pub toggles: HashMap<u8, u8>,
    /// Launch content per (profile, fwKey).
    pub launch: HashMap<(u8, u8), Vec<u8>>,
    /// Reject packets whose payload this predicate matches.
    pub nak_if: Option<PacketFilter>,
    /// Drop packets whose payload this predicate matches (no reply → timeout).
    pub ignore_if: Option<PacketFilter>,
}

impl EverestSim {
    pub fn new(variant: Variant) -> Self {
        let mut ext = ExtendInfo::from_dock(&Default::default());
        ext.dock_plug = 1;
        ext.numpad_plug = 2;
        EverestSim {
            variant,
            profile: 1,
            menu: 0,
            ext,
            clock_digital: true,
            clock_12h: false,
            toggles: HashMap::new(),
            launch: HashMap::new(),
            nak_if: None,
            ignore_if: None,
        }
    }

    fn fw_key_for(&self, key: u8) -> Option<u8> {
        let layout = match self.variant {
            Variant::EverestMax => tables::everest_max_layout(),
            Variant::MacroPad => tables::macropad_layout(),
        };
        layout.iter().find(|r| r.led == key as u16 && r.fw_key != 0).map(|r| r.fw_key as u8)
    }

    /// Reply to one written payload.
    pub fn respond(&mut self, p: &Packet) -> Vec<Vec<u8>> {
        if self.ignore_if.as_ref().is_some_and(|f| f(p)) {
            return vec![];
        }
        if self.nak_if.as_ref().is_some_and(|f| f(p)) {
            return vec![packet(&[0xFF, 0xAA, p[0], p[1]]).to_vec()];
        }
        // Like the firmware (fw 0x57): `14 2C 00 01 <chunk>` is a per-key colour chunk and
        // an out-of-range chunk number is rejected.
        if p[..4] == [0x14, 0x2C, 0x00, 0x01] && p[4] as usize >= (self.variant.led_count() * 3).div_ceil(57) {
            return vec![packet(&[0xFF, 0xAA, 0x00, 0x00]).to_vec()];
        }
        let mut r = *p;
        match (p[0], p[1], p[2], p[3]) {
            (0x11, 0x00, _, _) => {
                r = packet(&[0x11, 0x00, 0x00, 0x00, 0x05, 0x01, 0x00, 0x00, 0x06, 0x00, self.profile, self.menu]);
            }
            (0x11, 0x12, _, _) => r[4] = 0,
            (0x11, 0x14, 0x00, 0x00) => r[4..33].copy_from_slice(&self.ext.to_bytes()),
            (0x11, 0x14, 0x00, 0x01) => {
                if self.variant == Variant::EverestMax {
                    let mut e = ExtendInfo::from_bytes(&p[4..33]);
                    e.dock_plug = self.ext.dock_plug;
                    e.numpad_plug = self.ext.numpad_plug;
                    e.menu_index = self.ext.menu_index;
                    self.ext = e;
                }
            }
            (0x11, 0x84, 0x00, 0x00) => {
                r[11] = self.clock_digital as u8;
                r[12] = self.clock_12h as u8;
            }
            (0x12, code, 0x00, 0x00) if code >= 0x06 => r[4] = *self.toggles.get(&code).unwrap_or(&0),
            (0x12, code, 0x00, 0x01) if code >= 0x06 => {
                self.toggles.insert(code, p[4]);
            }
            (0x14, 0x00, _, _) => {
                self.profile = p[4];
                self.menu = p[5];
            }
            (0x13, 0x55, _, _) => self.menu = p[4],
            (0x17, src, len, seq) => {
                let e = self.launch.entry((self.profile, src)).or_default();
                if seq <= 1 {
                    e.clear();
                }
                e.extend_from_slice(&p[4..4 + len as usize]);
            }
            (0x14, 0x20, src, _) | (0x14, 0x21, src, _) => {
                self.launch.remove(&(self.profile, src));
            }
            (0x11, 0x02, 0x00, 0x01) => return self.key_content(p),
            _ => {}
        }
        vec![r.to_vec()]
    }

    fn key_content(&self, p: &Packet) -> Vec<Vec<u8>> {
        let content = self.fw_key_for(p[5]).and_then(|fw| self.launch.get(&(p[4], fw)));
        let Some(data) = content else {
            return vec![packet(&[0xFF, 0xAA, 0xFF, 0x00]).to_vec()];
        };
        let mut all = vec![0u8; 5];
        all[0..2].copy_from_slice(&(data.len() as u16).to_le_bytes());
        all[2] = 7;
        all.extend_from_slice(data);
        let parts: Vec<&[u8]> = all.chunks(59).collect();
        let n = parts.len();
        parts
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut r = packet(&[0x11, 0x02, 0x00, 0x01, (n - 1 - i) as u8]);
                r[5..5 + s.len()].copy_from_slice(s);
                r.to_vec()
            })
            .collect()
    }

    /// Key-state bitmap report with the given usages held.
    pub fn bitmap(usages: &[u16]) -> Vec<u8> {
        let mut p = packet(&[0x01]);
        for &u in usages {
            p[1 + u as usize / 8] |= 1 << (u % 8);
        }
        p.to_vec()
    }
}

/// A simulated Everest Max or MacroPad node.
pub fn everest(variant: Variant) -> (Arc<FakeHid>, Arc<Mutex<EverestSim>>) {
    let sim = Arc::new(Mutex::new(EverestSim::new(variant)));
    let s2 = sim.clone();
    let hid = FakeHid::new(move |op| match op {
        WireOp::Write(d) => s2.lock().unwrap().respond(&crate::link::to_packet(&d[1..])),
        WireOp::SetFeature(d) if d.len() > 3 && d[1] == 0xAA && d[2] == 0x55 && d[3] == 0x80 => {
            let mut r = vec![0u8; 65];
            r[1..4].copy_from_slice(&[0xAA, 0x55, 0x80]);
            r[5..7].copy_from_slice(&0x3282u16.to_le_bytes());
            r[7..9].copy_from_slice(&1u16.to_le_bytes());
            r[9..11].copy_from_slice(&0x0105u16.to_le_bytes());
            vec![r]
        }
        _ => vec![],
    });
    (hid, sim)
}

// ---------------------------------------------------------------------------------------
// DisplayPad simulator

#[derive(Default)]
pub struct DisplayPadSim {
    pub pending: Option<(usize, bool)>,
    pub received_blocks: usize,
    pub uploads: Vec<Packet>,
    pub brightness: u8,
}

pub fn displaypad() -> (Arc<FakeHid>, Arc<FakeHid>, Arc<Mutex<DisplayPadSim>>) {
    let sim = Arc::new(Mutex::new(DisplayPadSim { brightness: 80, ..Default::default() }));
    let (inner, _) = everest(Variant::MacroPad);
    let s1 = sim.clone();
    let cmd = FakeHid::new(move |op| match op {
        WireOp::Write(d) if d.len() > 1 && d[1] == 0x21 => {
            let p = crate::link::to_packet(&d[1..]);
            let mut s = s1.lock().unwrap();
            let bytes = if p[3] == 0x01 { u16::from_le_bytes([p[4], p[5]]) as usize * 512 } else { p[5] as usize * 512 };
            s.pending = Some((bytes.div_ceil(1024), p[1] == 0x01));
            s.received_blocks = 0;
            s.uploads.push(p);
            let mut echo = p;
            echo[2] = 0;
            echo[3] = 0;
            vec![echo.to_vec()]
        }
        WireOp::Write(d) if d.len() > 2 && d[1] == 0x12 && d[2] == 0x00 => {
            let mut r = crate::link::to_packet(&d[1..]);
            r[4] = s1.lock().unwrap().brightness;
            vec![r.to_vec()]
        }
        other => {
            // Everything else behaves like the Everest family.
            let h = &inner;
            match other {
                WireOp::Write(d) => {
                    h.write(d).ok();
                    let mut out = Vec::new();
                    while let Ok(Some(r)) = h.read(Duration::from_millis(0)) {
                        out.push(r);
                    }
                    out
                }
                _ => vec![],
            }
        }
    });
    let cmd_weak = Arc::downgrade(&cmd);
    let s2 = sim.clone();
    let bulk = FakeHid::new(move |op| {
        if let WireOp::Write(_) = op {
            let mut s = s2.lock().unwrap();
            s.received_blocks += 1;
            if let Some((n, stored)) = s.pending {
                if s.received_blocks >= n {
                    s.pending = None;
                    let done = if stored { [0x21, 0x01, 0x02, 0x00] } else { [0x21, 0x00, 0xFF, 0xFF] };
                    if let Some(c) = cmd_weak.upgrade() {
                        c.inject(packet(&done).to_vec());
                    }
                }
            }
        }
        vec![]
    });
    (cmd, bulk, sim)
}

// ---------------------------------------------------------------------------------------
// Everest 60 simulator

#[derive(Default)]
pub struct Everest60Sim {
    pub profile: u8,
    pub effect: u8,
    pub numpad: u8,
    pub keys: VecDeque<(u8, bool)>,
    pub last: Vec<u8>,
}

pub fn everest60() -> (Arc<FakeHid>, Arc<Mutex<Everest60Sim>>) {
    let sim = Arc::new(Mutex::new(Everest60Sim { profile: 1, effect: 1, numpad: 2, ..Default::default() }));
    let s = sim.clone();
    let hid = FakeHid::new(move |op| {
        let mut st = s.lock().unwrap();
        match op {
            WireOp::SetFeature(d) => {
                st.last = d[1..].to_vec();
                vec![]
            }
            WireOp::GetFeature(id) => {
                let req = st.last.clone();
                let mut r = vec![0u8; 65];
                r[0] = *id;
                if req.is_empty() {
                    return vec![r];
                }
                r[1] = req[0];
                r[2..5].copy_from_slice(&sherpa_core::everest60::MAGIC);
                let set = |r: &mut Vec<u8>, i: usize, v: u32| r[5 + 4 * i..9 + 4 * i].copy_from_slice(&v.to_le_bytes());
                match req[0] {
                    0x07 => {
                        set(&mut r, 0, 0x0102);
                        set(&mut r, 2, 6);
                        set(&mut r, 3, st.effect as u32);
                        set(&mut r, 4, st.profile as u32);
                        set(&mut r, 5, st.effect as u32);
                    }
                    0x08 => {
                        r[5] = st.numpad;
                        let n = st.keys.len().min(26);
                        r[6] = n as u8;
                        for i in 0..n {
                            let (k, down) = st.keys.pop_front().unwrap();
                            r[7 + 2 * i] = k;
                            r[8 + 2 * i] = down as u8;
                        }
                        r[63] = st.profile;
                        r[64] = st.effect;
                    }
                    0x16 => {
                        st.profile = req[4];
                        st.effect = req[8];
                        set(&mut r, 0, 1);
                    }
                    _ => set(&mut r, 0, 1),
                }
                vec![r]
            }
            WireOp::Write(_) => vec![],
        }
    });
    (hid, sim)
}

// ---------------------------------------------------------------------------------------
// Makalu 67 simulator

#[derive(Default)]
pub struct MakaluSim {
    pub profile: u8,
    pub last: Vec<u8>,
    pub bulk: HashMap<(u8, u8), Vec<u8>>,
    pub busy_polls: u32,
}

pub fn makalu() -> (Arc<FakeHid>, Arc<Mutex<MakaluSim>>) {
    let sim = Arc::new(Mutex::new(MakaluSim { profile: 1, ..Default::default() }));
    let s = sim.clone();
    let hid = FakeHid::new(move |op| {
        let mut st = s.lock().unwrap();
        match op {
            WireOp::SetFeature(d) if d[0] == 0xA0 => {
                st.bulk.insert((d[5], d[2]), d[16..1040].to_vec());
                vec![]
            }
            WireOp::SetFeature(d) => {
                st.last = d.clone();
                if d[1] == 0x0B && d[2] == 0x04 {
                    st.profile = d[5];
                }
                vec![]
            }
            WireOp::GetFeature(0xA0) => {
                let key = (st.last.get(5).copied().unwrap_or(0), st.last.get(2).copied().unwrap_or(0));
                let mut r = vec![0u8; 1041];
                r[0] = 0xA0;
                if let Some(b) = st.bulk.get(&key) {
                    r[16..16 + b.len()].copy_from_slice(b);
                }
                vec![r]
            }
            WireOp::GetFeature(_) => {
                let mut r = vec![0u8; 64];
                r[0] = 0xA1;
                if st.busy_polls > 0 {
                    st.busy_polls -= 1;
                    r[1] = 3;
                    return vec![r];
                }
                r[1] = 1;
                match (st.last.get(1).copied(), st.last.get(2).copied()) {
                    (Some(0x02), _) => r[16..18].copy_from_slice(&0x0110u16.to_le_bytes()),
                    (Some(0x0B), Some(0x03)) => r[16] = st.profile,
                    (Some(0x0B), Some(0x09)) => r[16] = 5,
                    _ => {}
                }
                vec![r]
            }
            WireOp::Write(_) => vec![],
        }
    });
    (hid, sim)
}

// ---------------------------------------------------------------------------------------
// Demo set for `sherpad --fake`

use crate::devices::Found;
use crate::link::Endpoint;
use sherpa_core::types::DeviceKind;

/// Handles to the simulators behind the demo devices.
pub struct Sims {
    pub everest_max: (Arc<FakeHid>, Arc<Mutex<EverestSim>>),
    pub macropad: (Arc<FakeHid>, Arc<Mutex<EverestSim>>),
    pub displaypad: (Arc<FakeHid>, Arc<FakeHid>, Arc<Mutex<DisplayPadSim>>),
    pub everest60: (Arc<FakeHid>, Arc<Mutex<Everest60Sim>>),
    pub makalu: (Arc<FakeHid>, Arc<Mutex<MakaluSim>>),
}

pub fn found(kind: DeviceKind, identity: &str, main: Arc<FakeHid>, bulk: Option<Arc<FakeHid>>) -> Found {
    let pid = match kind {
        DeviceKind::EverestMax => 1,
        DeviceKind::Makalu67 => 3,
        DeviceKind::Everest60 => 5,
        DeviceKind::MacroPad => 8,
        DeviceKind::DisplayPad => 9,
    };
    Found {
        key: crate::store::device_key(kind, identity),
        identity: identity.to_string(),
        kind,
        pid,
        product: format!("{} (simulated)", kind.name()),
        main: Endpoint::new(main, 0, Vec::new()),
        bulk: bulk.map(|b| Endpoint::new(b, 0, Vec::new())),
    }
}

pub fn demo_devices() -> (Vec<Found>, Sims) {
    let max = everest(Variant::EverestMax);
    let pad = everest(Variant::MacroPad);
    let dp = displaypad();
    let e60 = everest60();
    let mk = makalu();
    let found = vec![
        found(DeviceKind::EverestMax, "sim", max.0.clone(), None),
        found(DeviceKind::MacroPad, "sim-macropad-1", pad.0.clone(), None),
        found(DeviceKind::DisplayPad, "sim-displaypad-1", dp.0.clone(), Some(dp.1.clone())),
        found(DeviceKind::Everest60, "sim", e60.0.clone(), None),
        found(DeviceKind::Makalu67, "sim", mk.0.clone(), None),
    ];
    (found, Sims { everest_max: max, macropad: pad, displaypad: dp, everest60: e60, makalu: mk })
}
