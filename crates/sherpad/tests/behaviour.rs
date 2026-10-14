//! Behavioural acceptance tests against fake devices (spec 07 §2–3, 08–11 vectors).

use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sherpa_core::everest::Variant;
use sherpa_core::model::{BindingType, CustomLighting, DockSettings, KeyBinding, Macro, MacroEvent, MacroEventType, MouseButton};
use sherpa_core::types::{DeviceKind, LaunchKind, Rgb};
use sherpa_core::{hex, packet};
use sherpad::actions::RecordingExecutor;
use sherpad::bus::Signal;
use sherpad::daemon::{Daemon, Options, Timing};
use sherpad::devices::FakeBackend;
use sherpad::fake::{self, FakeHid, WireOp};
use sherpad::link::{Endpoint, Link};

fn timing() -> Timing {
    Timing {
        scan: Duration::from_millis(50),
        clock_delay: Duration::from_secs(3600),
        clock_period: Duration::from_secs(3600),
        feed: Duration::from_millis(50),
        dock_refresh: Duration::from_secs(3600),
        app_watch: Duration::from_millis(50),
        e60_poll: Duration::from_millis(5),
    }
}

struct Rig {
    d: Arc<Daemon>,
    x: Arc<RecordingExecutor>,
    backend: Arc<FakeBackend>,
}

fn rig_with(found: Vec<sherpad::devices::Found>, t: Timing) -> Rig {
    let x = Arc::new(RecordingExecutor::default());
    let backend = Arc::new(FakeBackend::new(found));
    let opts = Options { config_dir: None, timing: t, simulated: true };
    let d = Daemon::new(opts, Box::new(backend.clone()), x.clone(), None).unwrap();
    d.scan_once();
    Rig { d, x, backend }
}

fn max_rig(t: Timing) -> (Rig, Arc<FakeHid>, Arc<std::sync::Mutex<fake::EverestSim>>) {
    let (hid, sim) = fake::everest(Variant::EverestMax);
    let rig = rig_with(vec![fake::found(DeviceKind::EverestMax, "t", hid.clone(), None)], t);
    (rig, hid, sim)
}

fn wait_until(ms: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        if f() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    f()
}

fn signals(rx: &mut tokio::sync::broadcast::Receiver<Signal>) -> Vec<Signal> {
    let mut v = Vec::new();
    while let Ok(s) = rx.try_recv() {
        v.push(s);
    }
    v
}

fn starts(w: &[u8], prefix: &[u8]) -> bool {
    w.len() >= prefix.len() && &w[..prefix.len()] == prefix
}

fn binding(led: u16, kind: BindingType, value: &str) -> KeyBinding {
    KeyBinding { led, kind, value: value.into(), is_first_image_selected: true, ..Default::default() }
}

const DEV: &str = "everest-max";

// ---------------------------------------------------------------------------------------
// Engine (R3, R4)

#[test]
fn r3_nak_fails() {
    let hid = FakeHid::new(|op| match op {
        WireOp::Write(_) => vec![packet(&[0xFF, 0xAA]).to_vec()],
        _ => vec![],
    });
    let (link, _) = Link::start(Endpoint::new(hid, 0, vec![]), "t");
    let r = link.transact(&packet(&[0x11, 0x00]), 600);
    assert!(matches!(r, Err(sherpad::error::Error::Nak(_))), "{r:?}");
}

#[test]
fn r4_timeout_after_one_retry() {
    let hid = FakeHid::new(|_| vec![]);
    let (link, _) = Link::start(Endpoint::new(hid.clone(), 0, vec![]), "t");
    let t0 = Instant::now();
    let r = link.transact(&packet(&[0x11, 0x00]), 600);
    assert!(matches!(r, Err(sherpad::error::Error::Timeout(_))));
    assert!(t0.elapsed() >= Duration::from_millis(1150));
    assert_eq!(hid.writes().len(), 2, "sent once, retried once");
}

#[test]
fn events_are_skipped_while_waiting_and_report_ids_written() {
    let hid = FakeHid::new(|op| match op {
        WireOp::Write(d) => vec![packet(&[0x01, 0xFF]).to_vec(), packet(&[0x99]).to_vec(), d[1..].to_vec()],
        _ => vec![],
    });
    let (link, events) = Link::start(Endpoint::new(hid.clone(), 0x05, vec![]), "t");
    let r = link.transact(&packet(&[0x11, 0x12]), 600).unwrap();
    assert_eq!(&r[..2], &[0x11, 0x12]);
    assert_eq!(events.recv_timeout(Duration::from_secs(1)).unwrap()[0], 0x01);
    match &hid.log()[0] {
        WireOp::Write(d) => assert_eq!((d[0], d[1], d.len()), (0x05, 0x11, 65)),
        o => panic!("{o:?}"),
    }
}

// ---------------------------------------------------------------------------------------
// B1–B11

#[test]
fn b1_no_interleaving() {
    let (rig, hid, _) = max_rig(timing());
    hid.clear_log();
    let d1 = rig.d.clone();
    let d2 = rig.d.clone();
    let t1 = thread::spawn(move || {
        for i in 0..5 {
            let c = CustomLighting { leds: vec![Rgb(i, i, i); 126], ..Default::default() };
            d1.set_custom(DEV, 1, c, true).unwrap();
        }
    });
    let t2 = thread::spawn(move || {
        for _ in 0..40 {
            d2.set_binding(DEV, 1, binding(0, BindingType::Media, "Mute")).unwrap();
        }
    });
    t1.join().unwrap();
    t2.join().unwrap();
    let w = hid.writes();
    // Every per-key colour transfer is 7 contiguous chunks, each effect table 3.
    let mut i = 0;
    let mut transfers = 0;
    while i < w.len() {
        if starts(&w[i], &[0x14, 0x2C, 0x00, 0x01, 0x00, 0x4B]) {
            for k in 0..7u8 {
                assert!(
                    starts(&w[i + k as usize], &[0x14, 0x2C, 0x00, 0x01, k, 0x4B]),
                    "interleaved at {}: {}",
                    i + k as usize,
                    hex(&w[i + k as usize][..8])
                );
            }
            for k in 0..3u8 {
                assert!(starts(&w[i + 7 + k as usize], &[0x14, 0xA0, k, 0x01]));
            }
            assert!(starts(&w[i + 10], &[0x13, 0x55, 0x00, 0x00, 0x06]));
            transfers += 1;
            i += 11;
        } else {
            i += 1;
        }
    }
    assert_eq!(transfers, 5);
    assert_eq!(w.iter().filter(|p| starts(p, &[0x14, 0x20, 0x6e, 0x00, 0xb8])).count(), 40);
}

#[test]
fn b2_nak_mid_transfer_aborts_and_keeps_store() {
    let (rig, hid, sim) = max_rig(timing());
    sim.lock().unwrap().nak_if = Some(Box::new(|p| p[0] == 0x14 && p[1] == 0x2C && p[2] == 0 && p[3] == 1 && p[4] == 3));
    let before = rig.d.store().device(DEV).unwrap().profile(1).unwrap().clone();
    hid.clear_log();
    let c = CustomLighting { leds: vec![Rgb(9, 9, 9); 126], ..Default::default() };
    assert!(rig.d.set_custom(DEV, 1, c, true).is_err());
    let w = hid.writes();
    assert!(w.iter().any(|p| starts(p, &[0x14, 0x2C, 0x00, 0x01, 0x03])));
    assert!(!w.iter().any(|p| starts(p, &[0x14, 0x2C, 0x00, 0x01, 0x04])), "remaining chunks not sent");
    assert!(!w.iter().any(|p| starts(p, &[0x14, 0xA0])));
    assert!(!w.iter().any(|p| starts(p, &[0x13, 0x55])));
    assert_eq!(rig.d.store().device(DEV).unwrap().profile(1).unwrap(), &before, "store unchanged");
}

#[test]
fn b3_clock_sync_on_attach_and_periodically() {
    let t = Timing { clock_delay: Duration::from_millis(300), clock_period: Duration::from_millis(600), ..timing() };
    let (_rig, hid, _) = max_rig(t);
    let clocks = || hid.writes().iter().filter(|p| starts(p, &[0x11, 0x84, 0x00, 0x01])).count();
    thread::sleep(Duration::from_millis(150));
    assert_eq!(clocks(), 0, "not before the delay");
    assert!(wait_until(1000, || clocks() == 1));
    let w = hid.writes();
    let i = w.iter().position(|p| starts(p, &[0x11, 0x84, 0x00, 0x01])).unwrap();
    assert!(starts(&w[i - 1], &[0x11, 0x84, 0x00, 0x00]), "format read first");
    assert!(w[..i].iter().any(|p| starts(p, &[0x11, 0x80, 0x00, 0x00, 0x01])), "AP enable before clock sync");
    assert!(wait_until(1500, || clocks() >= 2));
}

#[test]
fn b4_pc_info_follows_dock_page() {
    let (hid, sim) = fake::everest(Variant::EverestMax);
    sim.lock().unwrap().ext.menu_index = 97;
    let _rig = rig_with(vec![fake::found(DeviceKind::EverestMax, "t", hid.clone(), None)], timing());
    let cpu = || hid.writes().iter().filter(|p| starts(p, &[0x11, 0x81, 0x00, 0x00])).count();
    assert!(wait_until(1000, || cpu() >= 3), "CPU pushed every tick");
    // The dock leaves the CPU page: MENU Change (index 107, usage 327).
    sim.lock().unwrap().ext.menu_index = 0;
    hid.inject(fake::EverestSim::bitmap(&[327]));
    hid.inject(fake::EverestSim::bitmap(&[]));
    thread::sleep(Duration::from_millis(200));
    let n = cpu();
    thread::sleep(Duration::from_millis(400));
    assert!(cpu() <= n + 1, "pushes stop when the page changes");
}

#[test]
fn b5_macro_rewritten_on_every_bound_key() {
    let (rig, hid, _) = max_rig(timing());
    let m = Macro {
        name: "M".into(),
        events: vec![MacroEvent::key(MacroEventType::Keydown, "A", 10), MacroEvent::key(MacroEventType::Keyup, "A", 0)],
        ..Default::default()
    };
    rig.d.put_macro(m.clone(), None).unwrap();
    rig.d.set_binding(DEV, 1, binding(0, BindingType::Macro, "M")).unwrap(); // Esc, fwKey 110
    rig.d.set_binding(DEV, 2, binding(9, BindingType::Macro, "M")).unwrap(); // F1, fwKey 112
    hid.clear_log();
    let mut m2 = m;
    m2.events.insert(0, MacroEvent::key(MacroEventType::Keydown, "B", 5));
    m2.events.insert(1, MacroEvent::key(MacroEventType::Keyup, "B", 5));
    rig.d.put_macro(m2, None).unwrap();
    let w = hid.writes();
    assert_eq!(w.iter().filter(|p| starts(p, &[0x14, 0x18, 0x01])).count(), 2, "two macro headers");
    assert!(w.iter().any(|p| starts(p, &[0x15, 0x6e, 0x05])));
    assert!(w.iter().any(|p| starts(p, &[0x15, 0x70, 0x05])));
    assert!(w.iter().any(|p| starts(p, &[0x14, 0x00, 0x00, 0x00, 0x02])), "switched to profile 2 to write it");
    assert!(starts(w.last().unwrap(), &[0x14, 0x00, 0x00, 0x00, 0x01]), "and back");
}

#[cfg(target_os = "linux")]
#[test]
fn b6_app_linked_profile() {
    let (rig, hid, _) = max_rig(timing());
    let dir = std::env::temp_dir().join(format!("bc-b6-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("bcfooapp");
    std::fs::copy("/bin/sleep", &exe).unwrap();
    rig.d.update_profile(DEV, 3, &serde_json::json!({"linkedExe": "bcfooapp", "revertOnExit": true})).unwrap();
    rig.d.start();
    thread::sleep(Duration::from_millis(200));
    hid.clear_log();
    let mut child = std::process::Command::new(&exe).arg("30").spawn().unwrap();
    let sw = |slot: u8| hid.writes().iter().any(|p| starts(p, &[0x14, 0x00, 0x00, 0x00, slot]));
    assert!(wait_until(3000, || sw(3)), "switched to slot 3");
    assert_eq!(rig.d.store().device(DEV).unwrap().selected_slot(), 3);
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(wait_until(3000, || sw(1)), "reverted to slot 1");
    assert_eq!(rig.d.store().device(DEV).unwrap().selected_slot(), 1);
    rig.d.stop();
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn b7_profile_key_event() {
    let (rig, _hid, _) = max_rig(timing());
    let mut rx = rig.d.bus.subscribe();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 172})).unwrap();
    assert_eq!(rig.d.store().device(DEV).unwrap().selected_slot(), 2);
    assert!(signals(&mut rx).iter().any(|s| s.signal == "ProfileChangedOnDevice" && s.data["slot"] == 2));
}

#[test]
fn b8_hotkey_switch_alternates() {
    let (rig, _hid, _) = max_rig(timing());
    let mut b = binding(0, BindingType::HotkeySwitch, "Ctrl+C");
    b.value2 = Some("Ctrl+V".into());
    rig.d.set_binding(DEV, 1, b).unwrap();
    for _ in 0..3 {
        rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
        thread::sleep(Duration::from_millis(150));
    }
    let downs: Vec<String> = rig.x.take().into_iter().filter(|c| c.ends_with(" down") && !c.contains("LCtrl")).collect();
    assert_eq!(downs, vec!["key C down", "key V down", "key C down"]);
}

#[test]
fn b9_media_key_needs_passthrough_page() {
    let (rig, _hid, sim) = max_rig(timing());
    rig.d.set_binding(DEV, 1, binding(35, BindingType::Url, "https://mountain.gg")).unwrap();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 35})).unwrap();
    thread::sleep(Duration::from_millis(150));
    assert!(rig.x.take().is_empty(), "no host action while menu index != 130");
    sim.lock().unwrap().ext.menu_index = 130;
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 35})).unwrap();
    assert!(wait_until(1000, || !rig.x.calls.lock().unwrap().is_empty()));
    assert_eq!(rig.x.take(), vec!["url https://mountain.gg"]);
}

#[test]
fn b10_duplicate_show_profile_rejected_before_device() {
    let (rig, hid, _) = max_rig(timing());
    hid.clear_log();
    let dock = DockSettings { profiles_shown: [1, 2, 3, 4, 0x14], ..Default::default() };
    assert!(rig.d.set_dock(DEV, dock).is_err());
    assert!(!hid.writes().iter().any(|p| starts(p, &[0x11, 0x14, 0x00, 0x01])));
    rig.d.set_dock(DEV, DockSettings::default()).unwrap();
    assert!(hid.writes().iter().any(|p| starts(p, &[0x11, 0x14, 0x00, 0x01])));
}

#[cfg(unix)]
#[test]
fn b11_other_uid_refused() {
    let me = unsafe { libc::geteuid() };
    assert!(sherpad::ipc::peer_allowed(me));
    assert!(!sherpad::ipc::peer_allowed(me.wrapping_add(1)));
    if me != 0 {
        return; // needs root to impersonate another user
    }
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    let dir = std::env::temp_dir().join(format!("bc-b11-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let sock = dir.join("d.sock").display().to_string();
    let (rig, _, _) = max_rig(timing());
    let d = rig.d.clone();
    let s2 = sock.clone();
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(sherpad::ipc::serve(d, &s2)).ok();
    });
    assert!(wait_until(2000, || std::path::Path::new(&sock).exists()));
    // Open the file permissions so only the peer-credential check stands in the way.
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o666)).unwrap();
    let bin = env!("CARGO_BIN_EXE_sherpad");
    let ok = std::process::Command::new(bin).args(["--socket", &sock, "--call", "GetStatus"]).output().unwrap();
    assert!(ok.status.success(), "same user is served: {}", String::from_utf8_lossy(&ok.stderr));
    let other = std::process::Command::new(bin).args(["--socket", &sock, "--call", "GetStatus"]).uid(65534).gid(65534).output().unwrap();
    assert!(!other.status.success(), "other uid refused");
    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------------------
// R9–R11 end to end, and the launch-content security rule

#[test]
fn r9_url_from_key_content() {
    let (rig, hid, _) = max_rig(timing());
    rig.d.set_binding(DEV, 1, binding(9, BindingType::Url, "http://a")).unwrap();
    // The binding was stored on the keyboard as launch content (G16 form).
    assert!(hid.writes().iter().any(|p| starts(p, &[0x17, 0x70, 0x09, 0x00, 0x02])));
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 9})).unwrap();
    assert!(wait_until(1000, || !rig.x.calls.lock().unwrap().is_empty()));
    assert_eq!(rig.x.take(), vec!["url http://a"]);
}

#[test]
fn r10_unknown_program_needs_approval() {
    let (rig, _hid, sim) = max_rig(timing());
    let mut rx = rig.d.bus.subscribe();
    let prog = if cfg!(windows) { "C:\\Windows\\notepad.exe" } else { "/bin/true" };
    // Content programmed on another machine: not in this host's store.
    let mut payload = vec![LaunchKind::Program as u8];
    payload.extend_from_slice(prog.as_bytes());
    sim.lock().unwrap().launch.insert((1, 110), payload);
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
    thread::sleep(Duration::from_millis(150));
    assert!(rig.x.take().is_empty(), "nothing runs");
    assert!(signals(&mut rx).iter().any(|s| s.signal == "LaunchApprovalRequired" && s.data["path"] == prog));
    rig.d.call("ApproveLaunch", &serde_json::json!({"path": prog})).unwrap();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
    if std::path::Path::new(prog).exists() {
        assert!(wait_until(1000, || !rig.x.calls.lock().unwrap().is_empty()));
        assert_eq!(rig.x.take(), vec![format!("program {prog}")]);
    }
    // A missing path never runs, even approved.
    sim.lock().unwrap().launch.insert((1, 110), [&[1u8][..], b"/no/such/prog"].concat());
    rig.d.call("ApproveLaunch", &serde_json::json!({"path": "/no/such/prog"})).unwrap();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
    thread::sleep(Duration::from_millis(150));
    assert!(rig.x.take().is_empty());
}

#[test]
fn r11_file_url_refused() {
    let (rig, _hid, sim) = max_rig(timing());
    let mut rx = rig.d.bus.subscribe();
    sim.lock().unwrap().launch.insert((1, 110), [&[2u8][..], b"file:///etc/passwd"].concat());
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
    assert!(wait_until(1000, || signals(&mut rx).iter().any(|s| s.signal == "ActionFailed")));
    assert!(rig.x.take().is_empty());
}

#[test]
fn multi_action_runs_steps_and_ignores_second_press() {
    let (rig, _hid, _) = max_rig(timing());
    let steps = serde_json::json!([
        {"type": "shortcut", "value": "A"},
        {"type": "delay", "value": "200"},
        {"type": "url", "value": "https://example.com"}
    ]);
    rig.d.set_binding(DEV, 1, binding(0, BindingType::MultiAction, &steps.to_string())).unwrap();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
    thread::sleep(Duration::from_millis(600));
    assert_eq!(rig.x.take(), vec!["key A down", "key A up", "url https://example.com"]);
}

#[test]
fn device_profile_wins_on_attach() {
    let (hid, sim) = fake::everest(Variant::EverestMax);
    sim.lock().unwrap().profile = 4;
    let rig = rig_with(vec![fake::found(DeviceKind::EverestMax, "t", hid, None)], timing());
    assert_eq!(rig.d.store().device(DEV).unwrap().selected_slot(), 4);
}

#[test]
fn detached_edits_are_marked_dirty_and_reapplied() {
    let (rig, hid, _) = max_rig(timing());
    hid.unplug();
    rig.backend.unplug(DEV);
    rig.d.scan_once();
    assert!(rig.d.attached(DEV).is_none());
    rig.d.set_binding(DEV, 1, binding(0, BindingType::Media, "Mute")).unwrap();
    assert!(rig.d.store().device(DEV).unwrap().profile(1).unwrap().dirty);
    let (hid2, _) = fake::everest(Variant::EverestMax);
    rig.backend.plug(fake::found(DeviceKind::EverestMax, "t", hid2.clone(), None));
    rig.d.scan_once();
    assert!(hid2.writes().iter().any(|p| starts(p, &[0x14, 0x20, 0x6e, 0x00, 0xb8])), "re-applied on attach");
    assert!(!rig.d.store().device(DEV).unwrap().profile(1).unwrap().dirty);
}

#[test]
fn lighting_apply_sends_then_saves() {
    let (rig, hid, _) = max_rig(timing());
    hid.clear_log();
    let mut l = sherpa_core::model::Lighting::new(sherpa_core::types::MenuIndex::Breathing);
    l.speed = 50;
    l.color1 = Rgb(0, 0xa0, 0xff);
    rig.d.set_lighting(DEV, 1, l, true).unwrap();
    let w = hid.writes();
    assert!(starts(&w[0], &sherpa_core::unhex("14 2c 01 01 03 64 00 ff ff 00 a0 ff")));
    assert!(starts(&w[1], &[0x13, 0x55, 0x00, 0x00, 0x03]));
    assert_eq!(rig.d.store().device(DEV).unwrap().profile(1).unwrap().active_menu(), sherpa_core::types::MenuIndex::Breathing);
}

// ---------------------------------------------------------------------------------------
// Other devices

#[test]
fn macropad_vectors_through_daemon() {
    let (hid, _) = fake::everest(Variant::MacroPad);
    let rig = rig_with(vec![fake::found(DeviceKind::MacroPad, "u1", hid.clone(), None)], timing());
    let dev = "macropad:u1";
    hid.clear_log();
    rig.d.set_binding(dev, 1, binding(8, BindingType::Media, "Mute")).unwrap();
    assert!(starts(&hid.writes()[0], &[0x14, 0x20, 0xaa, 0x00, 0xb8, 0x00]), "MP3");
    let mut c = CustomLighting::with_leds(12);
    c.leds = vec![Rgb(255, 0, 0); 12];
    hid.clear_log();
    rig.d.set_custom(dev, 1, c, true).unwrap();
    let w = hid.writes();
    assert!(starts(&w[1], &[0x14, 0x2c, 0x00, 0x01, 0x00, 0x4b, 0x00, 0xff, 0x00, 0x00]), "MP1");
    assert!(starts(&w[2], &[0x14, 0xa0, 0x00, 0x01]), "MP2");
    // Matrix numbers are used for key content reads.
    rig.d.set_binding(dev, 1, binding(17, BindingType::Url, "https://example.com")).unwrap();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": dev, "led": 17})).unwrap();
    assert!(wait_until(1000, || !rig.x.calls.lock().unwrap().is_empty()));
    assert_eq!(rig.x.take(), vec!["url https://example.com"]);
}

#[test]
fn everest60_profile_poll_and_keys() {
    let (hid, sim) = fake::everest60();
    let rig = rig_with(vec![fake::found(DeviceKind::Everest60, "t", hid.clone(), None)], timing());
    let dev = "everest-60";
    hid.clear_log();
    rig.d.select_profile(dev, 3).unwrap();
    let sets: Vec<Vec<u8>> =
        hid.log().into_iter().filter_map(|o| if let WireOp::SetFeature(d) = o { Some(d[1..].to_vec()) } else { None }).collect();
    assert!(sets.iter().any(|p| starts(p, &[0x16, 0x46, 0x23, 0xea, 0x03, 0, 0, 0, 0x00])), "E2-form switch");
    // A key bound to a URL on the Fn layer fires from the poll loop.
    let mut b = binding(30, BindingType::Url, "https://example.org");
    b.layer = 3;
    rig.d.set_binding(dev, 3, b).unwrap();
    let mut s = sim.lock().unwrap();
    s.keys.extend([(154, true), (30, true), (30, false), (154, false)]);
    drop(s);
    assert!(wait_until(2000, || !rig.x.calls.lock().unwrap().is_empty()));
    assert_eq!(rig.x.take(), vec!["url https://example.org"]);
    // The device changes profile itself (b62).
    let mut rx = rig.d.bus.subscribe();
    sim.lock().unwrap().profile = 5;
    let mut seen = Vec::new();
    assert!(wait_until(2000, || {
        seen.extend(signals(&mut rx));
        seen.iter().any(|s| s.signal == "ProfileChangedOnDevice" && s.data["slot"] == 5)
    }));
    assert_eq!(rig.d.store().device(dev).unwrap().selected_slot(), 5);
}

#[test]
fn makalu_button_and_events() {
    let (hid, sim) = fake::makalu();
    sim.lock().unwrap().busy_polls = 2;
    let rig = rig_with(vec![fake::found(DeviceKind::Makalu67, "t", hid.clone(), None)], timing());
    let dev = "makalu-67";
    hid.clear_log();
    let b = MouseButton { button: 4, kind: "url".into(), value: "https://example.net".into(), value2: None };
    rig.d.set_mouse_button(dev, 1, b).unwrap();
    let sets: Vec<Vec<u8>> = hid.log().into_iter().filter_map(|o| if let WireOp::SetFeature(d) = o { Some(d) } else { None }).collect();
    assert!(starts(&sets[0], &[0xa1, 0x0a, 0x00, 0x00, 0x00, 0x01, 0x04]));
    assert_eq!((sets[0][16], sets[0][22]), (0x24, 0x0f));
    assert_eq!(sets[1][0], 0xA0, "URL goes through the bulk channel");
    // Button press event: run browser (b4 = button, b5 = profile as in the bulk read).
    hid.inject(vec![0x00, 0x24, 0, 0, 4, 1, 0, 0, 0]);
    assert!(wait_until(2000, || !rig.x.calls.lock().unwrap().is_empty()));
    assert_eq!(rig.x.take(), vec!["url https://example.net"]);
    // Profile changed on the mouse.
    thread::sleep(Duration::from_millis(60));
    hid.inject(vec![0x00, 0x01, 3, 0, 0, 0, 0, 0, 0]);
    assert!(wait_until(2000, || rig.d.store().device(dev).unwrap().selected_slot() == 3));
}

#[test]
fn displaypad_init_layers_and_actions() {
    let (cmd, bulk, sim) = fake::displaypad();
    let rig = rig_with(vec![fake::found(DeviceKind::DisplayPad, "u1", cmd.clone(), Some(bulk.clone()))], timing());
    let dev = "displaypad:u1";
    {
        let s = sim.lock().unwrap();
        assert!(starts(&s.uploads[0], &sherpa_core::unhex("21 00 00 01 65 04 00 00 00 00 1f 03 ef 00")), "black panel first");
        assert_eq!(s.uploads.len(), 13, "panel + 12 icons");
        assert!(starts(&s.uploads[1], &sherpa_core::unhex("21 00 00 00 00 3d 00 00 65 65")));
    }
    assert_eq!(bulk.writes().len(), 563 + 12 * 31);
    // Folder with a child layer; the child gets a Back key on icon 0 (matrix 8).
    let child = rig.d.add_layer(dev, 1, 0, "Apps".into()).unwrap();
    rig.d.set_binding(dev, 1, binding(17, BindingType::DisplayFolder, &child.to_string())).unwrap();
    let mut sc = binding(26, BindingType::Shortcut, "Ctrl+Alt+T");
    sc.layer_id = child;
    rig.d.set_binding(dev, 1, sc).unwrap();
    rig.d.call("SimulateKey", &serde_json::json!({"dev": dev, "led": 17})).unwrap();
    assert_eq!(rig.d.attached(dev).unwrap().live().display_layer, child);
    rig.d.call("SimulateKey", &serde_json::json!({"dev": dev, "led": 26})).unwrap();
    assert!(wait_until(1000, || rig.x.calls.lock().unwrap().len() >= 6));
    assert_eq!(rig.x.take()[2], "key T down");
    rig.d.call("SimulateKey", &serde_json::json!({"dev": dev, "led": 8})).unwrap();
    assert_eq!(rig.d.attached(dev).unwrap().live().display_layer, 0, "Back returns to the parent");
}

#[test]
fn unplug_detaches_and_signals() {
    let (rig, hid, _) = max_rig(timing());
    let mut rx = rig.d.bus.subscribe();
    hid.unplug();
    assert!(wait_until(1000, || {
        rig.d.scan_once();
        rig.d.attached(DEV).is_none()
    }));
    assert!(signals(&mut rx).iter().any(|s| s.signal == "DeviceDetached"));
    let ids: HashSet<String> = rig.d.devices.read().unwrap().keys().cloned().collect();
    assert!(ids.is_empty());
}

// ---------------------------------------------------------------------------------------
// Launch approval is tied to the binding's kind, arguments, button and profile

#[cfg(unix)]
#[test]
fn approval_requires_same_kind_and_arguments() {
    let (rig, _hid, sim) = max_rig(timing());
    let mut rx = rig.d.bus.subscribe();
    let press = |content: &[u8]| {
        sim.lock().unwrap().launch.insert((1, 110), content.to_vec());
        rig.d.call("SimulateKey", &serde_json::json!({"dev": DEV, "led": 0})).unwrap();
        thread::sleep(Duration::from_millis(150));
    };
    // Bound as a folder: the same path as a program is not approved.
    rig.d.set_binding(DEV, 1, binding(0, BindingType::Folder, "/bin/true")).unwrap();
    press(&[&[1u8][..], b"/bin/true"].concat());
    assert!(rig.x.take().is_empty());
    assert!(signals(&mut rx).iter().any(|s| s.signal == "LaunchApprovalRequired"));
    // Bound as a program without arguments: added arguments are not approved.
    rig.d.set_binding(DEV, 1, binding(0, BindingType::Program, "/bin/true")).unwrap();
    press(&[&[1u8][..], b"/bin/true~--evil"].concat());
    assert!(rig.x.take().is_empty());
    assert!(signals(&mut rx).iter().any(|s| s.signal == "LaunchApprovalRequired" && s.data["path"] == "/bin/true~--evil"));
    // The exact bound content runs.
    press(&[&[1u8][..], b"/bin/true"].concat());
    assert!(wait_until(1000, || !rig.x.calls.lock().unwrap().is_empty()));
    assert_eq!(rig.x.take(), vec!["program /bin/true"]);
}

#[cfg(unix)]
#[test]
fn makalu_approval_is_per_button_and_profile() {
    let (hid, sim) = fake::makalu();
    let rig = rig_with(vec![fake::found(DeviceKind::Makalu67, "t", hid.clone(), None)], timing());
    let dev = "makalu-67";
    let mut rx = rig.d.bus.subscribe();
    let b = MouseButton { button: 4, kind: "program".into(), value: "/bin/true".into(), value2: None };
    rig.d.set_mouse_button(dev, 1, b).unwrap();
    // The same path programmed on button 5 elsewhere is not approved by button 4's binding.
    let mut path = b"/bin/true".to_vec();
    path.resize(1024, 0);
    sim.lock().unwrap().bulk.insert((1, 5), path.clone());
    hid.inject(vec![0x00, 0x23, 0, 0, 5, 1, 0, 0, 0]);
    assert!(wait_until(1000, || signals(&mut rx).iter().any(|s| s.signal == "LaunchApprovalRequired")));
    assert!(rig.x.take().is_empty());
    // Nor in another profile.
    sim.lock().unwrap().bulk.insert((2, 4), path);
    thread::sleep(Duration::from_millis(60));
    hid.inject(vec![0x00, 0x23, 0, 0, 4, 2, 0, 0, 0]);
    assert!(wait_until(1000, || signals(&mut rx).iter().any(|s| s.signal == "LaunchApprovalRequired")));
    assert!(rig.x.take().is_empty());
    // The bound button in the active profile runs.
    thread::sleep(Duration::from_millis(60));
    hid.inject(vec![0x00, 0x23, 0, 0, 4, 1, 0, 0, 0]);
    assert!(wait_until(1000, || !rig.x.calls.lock().unwrap().is_empty()));
    assert_eq!(rig.x.take(), vec!["program /bin/true"]);
}
