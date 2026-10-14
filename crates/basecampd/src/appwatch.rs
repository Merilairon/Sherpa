//! App-linked profiles (F-PRO-5): switch when a linked executable starts, revert when it
//! exits. Matching is on the executable file name only.

use std::collections::{HashMap, HashSet};

/// A profile linked to an executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub device: String,
    pub slot: u8,
    pub exe: String,
    pub revert: bool,
}

/// A profile switch requested by the watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub device: String,
    pub slot: u8,
}

#[derive(Default)]
pub struct AppWatcher {
    running: HashSet<String>,
    /// device → (exe that caused the switch, slot to revert to, revert flag)
    active: HashMap<String, (String, u8, bool)>,
}

/// File name of an executable path, lower-cased (Windows names are case-insensitive).
pub fn exe_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_lowercase()
}

impl AppWatcher {
    /// Feed the set of running executable names; `selected` gives each device's current
    /// slot. Returns the switches to perform.
    pub fn tick(&mut self, running: &HashSet<String>, links: &[Link], selected: &dyn Fn(&str) -> Option<u8>) -> Vec<Switch> {
        let mut out = Vec::new();
        let started: Vec<&String> = running.difference(&self.running).collect();
        let exited: Vec<String> = self.running.difference(running).cloned().collect();
        for name in started {
            for l in links.iter().filter(|l| exe_name(&l.exe) == *name) {
                let Some(cur) = selected(&l.device) else { continue };
                if cur == l.slot {
                    continue;
                }
                let prev = self.active.get(&l.device).map(|a| a.1).unwrap_or(cur);
                self.active.insert(l.device.clone(), (name.clone(), prev, l.revert));
                out.push(Switch { device: l.device.clone(), slot: l.slot });
            }
        }
        for name in exited {
            let devs: Vec<String> = self.active.iter().filter(|(_, a)| a.0 == name).map(|(d, _)| d.clone()).collect();
            for d in devs {
                let (_, prev, revert) = self.active.remove(&d).unwrap();
                if revert {
                    out.push(Switch { device: d, slot: prev });
                }
            }
        }
        self.running = running.clone();
        out
    }
}

/// Executable file names of all running processes.
pub fn running_exes(sys: &mut sysinfo::System) -> HashSet<String> {
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes()
        .values()
        .filter_map(|p| {
            p.exe()
                .and_then(|e| e.file_name())
                .map(|n| n.to_string_lossy().to_lowercase())
                .or_else(|| Some(p.name().to_string_lossy().to_lowercase()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b6_switch_and_revert() {
        let mut w = AppWatcher::default();
        let links = vec![Link { device: "kb".into(), slot: 3, exe: "/usr/bin/foo".into(), revert: true }];
        let sel = |_: &str| Some(1u8);
        assert!(w.tick(&HashSet::new(), &links, &sel).is_empty());
        let running: HashSet<String> = ["foo".to_string()].into();
        assert_eq!(w.tick(&running, &links, &sel), vec![Switch { device: "kb".into(), slot: 3 }]);
        assert!(w.tick(&running, &links, &|_| Some(3)).is_empty());
        assert_eq!(w.tick(&HashSet::new(), &links, &|_| Some(3)), vec![Switch { device: "kb".into(), slot: 1 }]);
    }

    #[test]
    fn name_only() {
        assert_eq!(exe_name(r"C:\Games\Foo.EXE"), "foo.exe");
        assert_eq!(exe_name("/opt/x/bar"), "bar");
    }
}
