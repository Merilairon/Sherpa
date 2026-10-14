//! Key-state bitmap input reports (`01 …`, spec 04 §10 and 05 §1).

use crate::Packet;

/// Tracks the previous bitmap and reports changes.
#[derive(Debug, Clone)]
pub struct KeyState {
    prev: [u8; 64],
}

impl Default for KeyState {
    fn default() -> Self {
        KeyState { prev: [0; 64] }
    }
}

/// One changed key: `(hid usage, pressed)`.
pub type Change = (u16, bool);

/// Is usage `u` set in a bitmap payload? Bit *u* lives in byte `1 + u/8`, bit `u % 8`.
pub fn is_set(p: &Packet, usage: u16) -> bool {
    let byte = 1 + usage as usize / 8;
    byte < 64 && p[byte] & (1 << (usage % 8)) != 0
}

impl KeyState {
    /// Diff a new `01` report against the previous one.
    #[allow(clippy::needless_range_loop)]
    pub fn update(&mut self, p: &Packet) -> Vec<Change> {
        let mut out = Vec::new();
        for byte in 1..64 {
            let diff = self.prev[byte] ^ p[byte];
            if diff == 0 {
                continue;
            }
            for bit in 0..8 {
                if diff & (1 << bit) != 0 {
                    let usage = ((byte - 1) * 8 + bit) as u16;
                    out.push((usage, p[byte] & (1 << bit) != 0));
                }
            }
        }
        self.prev = *p;
        out
    }

    pub fn reset(&mut self) {
        self.prev = [0; 64];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{packet, tables};

    #[test]
    fn r7_r8() {
        let mut ks = KeyState::default();
        let mut down = packet(&[0x01]);
        down[6] = 0x02;
        let ch = ks.update(&down);
        assert_eq!(ch, vec![(0x29, true)]);
        let row = tables::layout_by_usage(tables::everest_max_layout(), ch[0].0).unwrap();
        assert_eq!((row.index, row.label.as_str()), (0, "Esc"));
        assert_eq!(ks.update(&packet(&[0x01])), vec![(0x29, false)]);
        assert!(ks.update(&packet(&[0x01])).is_empty());
    }
}
