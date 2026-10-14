//! Synthetic input: a uinput virtual device on Linux, SendInput on Windows.

use sherpa_core::keys::HostKey;

use crate::error::Result;

pub use imp::Injector;

/// Name of the virtual input device (Linux); the macro recorder ignores it.
pub const VIRTUAL_DEVICE_NAME: &str = "sherpad virtual input";

#[cfg(target_os = "linux")]
mod imp {
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    use std::thread;
    use std::time::Duration;

    use sherpa_core::keys::{evdev_btn, HostKey};

    use crate::error::{Error, Result};

    const EV_SYN: u16 = 0;
    const EV_KEY: u16 = 1;
    const EV_REL: u16 = 2;
    const REL_HWHEEL: u16 = 6;
    const REL_WHEEL: u16 = 8;

    const fn ioc(dir: u64, nr: u64, size: u64) -> u64 {
        (dir << 30) | (size << 16) | ((b'U' as u64) << 8) | nr
    }
    const UI_DEV_CREATE: u64 = ioc(0, 1, 0);
    const UI_DEV_DESTROY: u64 = ioc(0, 2, 0);
    const UI_DEV_SETUP: u64 = ioc(1, 3, 92);
    const UI_SET_EVBIT: u64 = ioc(1, 100, 4);
    const UI_SET_KEYBIT: u64 = ioc(1, 101, 4);
    const UI_SET_RELBIT: u64 = ioc(1, 102, 4);

    #[repr(C)]
    struct UinputSetup {
        bustype: u16,
        vendor: u16,
        product: u16,
        version: u16,
        name: [u8; 80],
        ff_effects_max: u32,
    }

    pub struct Injector {
        file: File,
    }

    fn ioctl_int(fd: i32, req: u64, v: i32) -> std::io::Result<()> {
        if unsafe { libc::ioctl(fd, req as _, v) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    impl Injector {
        pub fn open() -> Result<Injector> {
            let file = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open("/dev/uinput").map_err(|e| {
                Error::Unsupported(format!("cannot open /dev/uinput ({e}); install the udev rule to allow synthetic input"))
            })?;
            let fd = file.as_raw_fd();
            let setup = || -> std::io::Result<()> {
                ioctl_int(fd, UI_SET_EVBIT, EV_KEY as i32)?;
                ioctl_int(fd, UI_SET_EVBIT, EV_REL as i32)?;
                ioctl_int(fd, UI_SET_EVBIT, EV_SYN as i32)?;
                for code in 1..=255 {
                    ioctl_int(fd, UI_SET_KEYBIT, code)?;
                }
                for code in evdev_btn::LEFT..=evdev_btn::EXTRA {
                    ioctl_int(fd, UI_SET_KEYBIT, code as i32)?;
                }
                ioctl_int(fd, UI_SET_RELBIT, REL_WHEEL as i32)?;
                ioctl_int(fd, UI_SET_RELBIT, REL_HWHEEL as i32)?;
                let mut name = [0u8; 80];
                let n = crate::actions::inject::VIRTUAL_DEVICE_NAME.as_bytes();
                name[..n.len()].copy_from_slice(n);
                let s = UinputSetup { bustype: 0x03, vendor: 0x3282, product: 0xBC00, version: 1, name, ff_effects_max: 0 };
                if unsafe { libc::ioctl(fd, UI_DEV_SETUP as _, &s as *const UinputSetup) } < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if unsafe { libc::ioctl(fd, UI_DEV_CREATE as _) } < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            };
            setup().map_err(|e| Error::Io(format!("uinput setup: {e}")))?;
            // Give the input stack time to register the new device.
            thread::sleep(Duration::from_millis(200));
            Ok(Injector { file })
        }

        fn emit(&mut self, ty: u16, code: u16, value: i32) -> Result<()> {
            let ev = libc::input_event { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, type_: ty, code, value };
            let bytes = unsafe { std::slice::from_raw_parts(&ev as *const _ as *const u8, std::mem::size_of::<libc::input_event>()) };
            self.file.write_all(bytes)?;
            Ok(())
        }

        fn syn(&mut self) -> Result<()> {
            self.emit(EV_SYN, 0, 0)
        }

        pub fn key(&mut self, key: &HostKey, down: bool) -> Result<()> {
            if key.evdev == 0 {
                return Err(Error::Unsupported(format!("key {} has no Linux key code", key.name)));
            }
            self.emit(EV_KEY, key.evdev, down as i32)?;
            self.syn()
        }

        pub fn mouse_button(&mut self, button: u8, down: bool) -> Result<()> {
            let code = match button {
                1 => evdev_btn::LEFT,
                2 => evdev_btn::RIGHT,
                3 => evdev_btn::MIDDLE,
                4 => evdev_btn::SIDE,
                5 => evdev_btn::EXTRA,
                b => return Err(Error::Invalid(format!("mouse button {b}"))),
            };
            self.emit(EV_KEY, code, down as i32)?;
            self.syn()
        }

        pub fn wheel(&mut self, amount: i32, horizontal: bool) -> Result<()> {
            self.emit(EV_REL, if horizontal { REL_HWHEEL } else { REL_WHEEL }, amount)?;
            self.syn()
        }
    }

    impl Drop for Injector {
        fn drop(&mut self) {
            unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY as _) };
        }
    }
}

#[cfg(windows)]
mod imp {
    use sherpa_core::keys::HostKey;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    use crate::error::{Error, Result};

    pub struct Injector;

    fn extended(vk: u32) -> bool {
        matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0x6F | 0xA3 | 0xA5 | 0xAD..=0xB3 | 3612)
    }

    fn send(inputs: &[INPUT]) -> Result<()> {
        let n = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if n as usize != inputs.len() {
            return Err(Error::Io("SendInput was blocked".into()));
        }
        Ok(())
    }

    impl Injector {
        pub fn open() -> Result<Injector> {
            Ok(Injector)
        }

        pub fn key(&mut self, key: &HostKey, down: bool) -> Result<()> {
            let vk = if key.vk == 3612 { 0x0D } else { key.vk };
            if vk > 0xFE {
                return Err(Error::Unsupported(format!("key {} has no Windows key code", key.name)));
            }
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if extended(key.vk) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if !down {
                flags |= KEYEVENTF_KEYUP;
            }
            let scan = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) } as u16;
            let i = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk as u16), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
            };
            send(&[i])
        }

        pub fn mouse_button(&mut self, button: u8, down: bool) -> Result<()> {
            let (flags, data) = match (button, down) {
                (1, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                (1, false) => (MOUSEEVENTF_LEFTUP, 0),
                (2, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                (2, false) => (MOUSEEVENTF_RIGHTUP, 0),
                (3, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                (3, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                (4, true) => (MOUSEEVENTF_XDOWN, 1),
                (4, false) => (MOUSEEVENTF_XUP, 1),
                (5, true) => (MOUSEEVENTF_XDOWN, 2),
                (5, false) => (MOUSEEVENTF_XUP, 2),
                (b, _) => return Err(Error::Invalid(format!("mouse button {b}"))),
            };
            self.mouse(flags, data)
        }

        pub fn wheel(&mut self, amount: i32, horizontal: bool) -> Result<()> {
            self.mouse(if horizontal { MOUSEEVENTF_HWHEEL } else { MOUSEEVENTF_WHEEL }, amount * 120)
        }

        fn mouse(&mut self, flags: MOUSE_EVENT_FLAGS, data: i32) -> Result<()> {
            let i = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
            };
            send(&[i])
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use sherpa_core::keys::HostKey;

    use crate::error::{Error, Result};

    pub struct Injector;

    impl Injector {
        pub fn open() -> Result<Injector> {
            Err(Error::Unsupported("synthetic input".into()))
        }
        pub fn key(&mut self, _: &HostKey, _: bool) -> Result<()> {
            unreachable!()
        }
        pub fn mouse_button(&mut self, _: u8, _: bool) -> Result<()> {
            unreachable!()
        }
        pub fn wheel(&mut self, _: i32, _: bool) -> Result<()> {
            unreachable!()
        }
    }
}

#[allow(dead_code)]
fn _assert_types(_: &HostKey) -> Result<()> {
    Ok(())
}
