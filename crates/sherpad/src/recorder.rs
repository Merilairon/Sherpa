//! Global macro recording (F-MAC-3): Linux evdev, Windows low-level hooks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use sherpa_core::model::{MacroEvent, MacroEventType};

use crate::error::Result;

/// One raw captured event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raw {
    Key { name: &'static str, down: bool },
    Button { button: u8, down: bool },
    Wheel { up: bool },
}

pub type Sink = Arc<Mutex<Vec<(Instant, Raw)>>>;

/// Recording stops collecting after this many events (a device macro holds 999).
pub const MAX_RECORDED: usize = 2000;

/// Append to the sink unless it is full.
fn record(sink: &Sink, at: Instant, raw: Raw) {
    let mut v = sink.lock().unwrap_or_else(|e| e.into_inner());
    if v.len() < MAX_RECORDED {
        v.push((at, raw));
    }
}

pub struct Recorder {
    stop: Arc<AtomicBool>,
    sink: Sink,
    handle: Option<JoinHandle<()>>,
    #[cfg(windows)]
    thread_id: u32,
}

/// Turn timestamped events into macro events: `delay` = ms until the next event, last = 0.
pub fn to_macro_events(raw: &[(Instant, Raw)]) -> Vec<MacroEvent> {
    let mut out = Vec::with_capacity(raw.len());
    for (i, (t, r)) in raw.iter().enumerate() {
        let delay = raw.get(i + 1).map(|(n, _)| n.duration_since(*t).as_millis() as u32).unwrap_or(0);
        let ev = match r {
            Raw::Key { name, down } => MacroEvent::key(if *down { MacroEventType::Keydown } else { MacroEventType::Keyup }, name, delay),
            Raw::Button { button, down } => MacroEvent {
                kind: if *down { MacroEventType::Mousedown } else { MacroEventType::Mouseup },
                key: None,
                rawcode: None,
                button: Some(*button),
                rotation: None,
                delay_ms: delay,
            },
            Raw::Wheel { up } => MacroEvent {
                kind: MacroEventType::Wheel,
                key: None,
                rawcode: None,
                button: None,
                rotation: Some(if *up { 1 } else { -1 }),
                delay_ms: delay,
            },
        };
        out.push(ev);
    }
    out
}

impl Recorder {
    pub fn start(keyboard: bool, mouse: bool) -> Result<Recorder> {
        let stop = Arc::new(AtomicBool::new(false));
        let sink: Sink = Arc::new(Mutex::new(Vec::new()));
        imp::start(keyboard, mouse, stop, sink)
    }

    pub fn stop(mut self) -> Vec<MacroEvent> {
        self.stop.store(true, Ordering::SeqCst);
        #[cfg(windows)]
        imp::wake(self.thread_id);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let raw = std::mem::take(&mut *self.sink.lock().unwrap());
        to_macro_events(&raw)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs::{self, File, OpenOptions};
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    use sherpa_core::keys::{self, evdev_btn};

    use super::{Raw, Recorder, Sink};
    use crate::error::{Error, Result};

    const EV_KEY: u16 = 1;
    const EV_REL: u16 = 2;
    const REL_WHEEL: u16 = 8;

    pub fn start(keyboard: bool, mouse: bool, stop: Arc<AtomicBool>, sink: Sink) -> Result<Recorder> {
        let mut files: Vec<File> = Vec::new();
        if let Ok(rd) = fs::read_dir("/dev/input") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("event") {
                    if let Ok(f) = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(e.path()) {
                        // Never record our own synthetic input.
                        if device_name(&f).as_deref() == Some(crate::actions::inject::VIRTUAL_DEVICE_NAME) {
                            continue;
                        }
                        files.push(f);
                    }
                }
            }
        }
        if files.is_empty() {
            return Err(Error::Unsupported("cannot read /dev/input/event* — add your user to the 'input' group to record macros".into()));
        }
        let s2 = stop.clone();
        let sink2 = sink.clone();
        let handle = thread::Builder::new()
            .name("recorder".into())
            .spawn(move || run(files, keyboard, mouse, s2, sink2))
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Recorder { stop, sink, handle: Some(handle) })
    }

    /// `EVIOCGNAME`: the input device's name.
    fn device_name(f: &File) -> Option<String> {
        let mut buf = [0u8; 256];
        let req = (2u64 << 30) | ((buf.len() as u64) << 16) | ((b'E' as u64) << 8) | 0x06;
        let n = unsafe { libc::ioctl(f.as_raw_fd(), req as _, buf.as_mut_ptr()) };
        if n <= 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }

    fn run(mut files: Vec<File>, keyboard: bool, mouse: bool, stop: Arc<AtomicBool>, sink: Sink) {
        let size = std::mem::size_of::<libc::input_event>();
        let mut buf = vec![0u8; size * 64];
        while !stop.load(Ordering::SeqCst) {
            if files.is_empty() {
                // Every device went away; wait for the user to stop recording.
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            let mut fds: Vec<libc::pollfd> =
                files.iter().map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
            let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 100) };
            if r <= 0 {
                continue;
            }
            let mut gone = Vec::new();
            for (i, pfd) in fds.iter().enumerate() {
                // An unplugged device reports POLLERR/POLLHUP forever: stop watching it.
                if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    gone.push(i);
                    continue;
                }
                if pfd.revents & libc::POLLIN == 0 {
                    continue;
                }
                let n = match files[i].read(&mut buf) {
                    Ok(0) => {
                        gone.push(i);
                        continue;
                    }
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        gone.push(i);
                        continue;
                    }
                };
                let now = Instant::now();
                for chunk in buf[..n].chunks_exact(size) {
                    let ev: libc::input_event = unsafe { std::ptr::read_unaligned(chunk.as_ptr() as *const libc::input_event) };
                    let raw = match (ev.type_, ev.code, ev.value) {
                        (EV_KEY, _, 2) => None, // autorepeat
                        (EV_KEY, c, v) if (evdev_btn::LEFT..=evdev_btn::MIDDLE).contains(&c) && mouse => {
                            let button = match c {
                                evdev_btn::LEFT => 1,
                                evdev_btn::RIGHT => 2,
                                _ => 3,
                            };
                            Some(Raw::Button { button, down: v == 1 })
                        }
                        (EV_KEY, c, v) if c < 0x100 && keyboard => keys::by_evdev(c).map(|k| Raw::Key { name: k.name, down: v == 1 }),
                        (EV_REL, REL_WHEEL, v) if mouse && v != 0 => Some(Raw::Wheel { up: v > 0 }),
                        _ => None,
                    };
                    if let Some(raw) = raw {
                        super::record(&sink, now, raw);
                    }
                }
            }
            for i in gone.into_iter().rev() {
                files.remove(i);
            }
        }
        files.clear();
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Instant;

    use sherpa_core::keys;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::*;

    use super::{Raw, Recorder, Sink};
    use crate::error::{Error, Result};

    static SINK: Mutex<Option<(Sink, bool, bool)>> = Mutex::new(None);

    fn push(r: Raw) {
        if let Some((s, _, _)) = SINK.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            super::record(s, Instant::now(), r);
        }
    }

    fn wants() -> (bool, bool) {
        SINK.lock().unwrap().as_ref().map(|(_, k, m)| (*k, *m)).unwrap_or((false, false))
    }

    unsafe extern "system" fn kb_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && wants().0 {
            let k = &*(l.0 as *const KBDLLHOOKSTRUCT);
            // Skip synthetic input (including our own SendInput).
            if k.flags.0 & LLKHF_INJECTED.0 != 0 {
                return CallNextHookEx(None, code, w, l);
            }
            let down = matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let mut vk = k.vkCode;
            if vk == 0x0D && k.flags.0 & LLKHF_EXTENDED.0 != 0 {
                vk = 3612;
            }
            if let Some(h) = keys::by_vk(vk) {
                push(Raw::Key { name: h.name, down });
            }
        }
        CallNextHookEx(None, code, w, l)
    }

    unsafe extern "system" fn mouse_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && wants().1 {
            let m = &*(l.0 as *const MSLLHOOKSTRUCT);
            if m.flags & LLMHF_INJECTED != 0 {
                return CallNextHookEx(None, code, w, l);
            }
            let r = match w.0 as u32 {
                WM_LBUTTONDOWN => Some(Raw::Button { button: 1, down: true }),
                WM_LBUTTONUP => Some(Raw::Button { button: 1, down: false }),
                WM_RBUTTONDOWN => Some(Raw::Button { button: 2, down: true }),
                WM_RBUTTONUP => Some(Raw::Button { button: 2, down: false }),
                WM_MBUTTONDOWN => Some(Raw::Button { button: 3, down: true }),
                WM_MBUTTONUP => Some(Raw::Button { button: 3, down: false }),
                WM_MOUSEWHEEL => Some(Raw::Wheel { up: ((m.mouseData >> 16) as i16) > 0 }),
                _ => None,
            };
            if let Some(r) = r {
                push(r);
            }
        }
        CallNextHookEx(None, code, w, l)
    }

    pub fn start(keyboard: bool, mouse: bool, stop: Arc<AtomicBool>, sink: Sink) -> Result<Recorder> {
        *SINK.lock().unwrap() = Some((sink.clone(), keyboard, mouse));
        let (tx, rx) = mpsc::channel();
        let s2 = stop.clone();
        let handle = thread::Builder::new()
            .name("recorder".into())
            .spawn(move || unsafe {
                let tid = GetCurrentThreadId();
                let inst = GetModuleHandleW(None).map(|h| windows::Win32::Foundation::HINSTANCE(h.0)).unwrap_or_default();
                let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), inst, 0);
                let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), inst, 0);
                let _ = tx.send(tid);
                let mut msg = MSG::default();
                while !s2.load(Ordering::SeqCst) && GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                if let Ok(h) = kb {
                    let _ = UnhookWindowsHookEx(h);
                }
                if let Ok(h) = ms {
                    let _ = UnhookWindowsHookEx(h);
                }
                *SINK.lock().unwrap() = None;
            })
            .map_err(|e| Error::Io(e.to_string()))?;
        let thread_id = rx.recv().map_err(|_| Error::Io("recorder thread failed".into()))?;
        Ok(Recorder { stop, sink, handle: Some(handle), thread_id })
    }

    pub fn wake(tid: u32) {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use super::{Recorder, Sink};
    use crate::error::{Error, Result};

    pub fn start(_: bool, _: bool, _: Arc<AtomicBool>, _: Sink) -> Result<Recorder> {
        Err(Error::Unsupported("macro recording".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn delays() {
        let t0 = Instant::now();
        let raw = vec![
            (t0, Raw::Key { name: "A", down: true }),
            (t0 + Duration::from_millis(30), Raw::Key { name: "A", down: false }),
            (t0 + Duration::from_millis(45), Raw::Wheel { up: false }),
        ];
        let ev = to_macro_events(&raw);
        assert_eq!(ev.iter().map(|e| e.delay_ms).collect::<Vec<_>>(), vec![30, 15, 0]);
        assert_eq!(ev[2].rotation, Some(-1));
    }
}
