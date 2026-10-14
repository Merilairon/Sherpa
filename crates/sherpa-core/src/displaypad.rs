//! DisplayPad protocol (spec 09): icon/panel image uploads and DisplayPad-only commands.

use crate::{err, packet, Packet, ProtoResult};

pub const ICON_SIZE: usize = 102;
pub const ICON_BYTES: usize = ICON_SIZE * ICON_SIZE * 3;
pub const PANEL_W: usize = 800;
pub const PANEL_H: usize = 240;
pub const PANEL_BYTES: usize = PANEL_W * PANEL_H * 3;
pub const BULK_BLOCK: usize = 1024;
pub const KEYS: u8 = 12;

/// Completion timeouts (ms).
pub const LIVE_TIMEOUT: u64 = 600;
pub const STORED_FIRST_TIMEOUT: u64 = 10_000;
pub const STORED_TIMEOUT: u64 = 2_000;

/// Frame data of a stored icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub profile: u8,
    pub count: u8,
    pub index: u8,
    pub interval_ms: u16,
}

/// Key icon header (§4.2). `stored = None` uploads live.
pub fn icon_header(icon: u8, len: usize, rect: (u8, u8, u8, u8), stored: Option<Frame>) -> ProtoResult<Packet> {
    if icon >= KEYS {
        return err("icon index must be 0..11");
    }
    let blocks = len.div_ceil(512);
    if blocks > 255 {
        return err("icon too large");
    }
    let mut p = packet(&[0x21]);
    match stored {
        None => p[4] = icon,
        Some(f) => {
            if f.profile > 0x0F {
                return err("profile out of range");
            }
            p[1] = 0x01;
            p[4] = (f.profile << 4) | (icon & 0x0F);
            p[10] = f.count;
            p[11] = f.index;
            p[12..14].copy_from_slice(&f.interval_ms.to_le_bytes());
        }
    }
    p[5] = blocks as u8;
    p[6] = rect.0;
    p[7] = rect.1;
    p[8] = rect.2;
    p[9] = rect.3;
    Ok(p)
}

/// Full-size icon header.
pub fn full_icon_header(icon: u8, stored: Option<Frame>) -> ProtoResult<Packet> {
    icon_header(icon, ICON_BYTES, (0, 0, 101, 101), stored)
}

/// Panel image header (§4.3), live.
pub fn panel_header(len: usize, left: u16, top: u16, right: u16, bottom: u16) -> Packet {
    let mut p = packet(&[0x21, 0x00, 0x00, 0x01]);
    p[4..6].copy_from_slice(&(len.div_ceil(512) as u16).to_le_bytes());
    p[6..8].copy_from_slice(&left.to_le_bytes());
    p[8..10].copy_from_slice(&top.to_le_bytes());
    p[10..12].copy_from_slice(&right.to_le_bytes());
    p[12..14].copy_from_slice(&bottom.to_le_bytes());
    p
}

/// Split pixel data into 1024-byte bulk blocks, zero-padding the last.
pub fn bulk_blocks(data: &[u8]) -> Vec<[u8; BULK_BLOCK]> {
    data.chunks(BULK_BLOCK)
        .map(|c| {
            let mut b = [0u8; BULK_BLOCK];
            b[..c.len()].copy_from_slice(c);
            b
        })
        .collect()
}

/// Completion check for an upload. Returns `Some(Ok)` on completion, `Some(Err)` when
/// rejected, `None` for an unrelated report.
pub fn completion(reply: &Packet, stored: bool) -> Option<Result<(), ()>> {
    if reply[0] == 0xFF && reply[1] == 0xAA {
        return Some(Err(()));
    }
    if reply[0] != 0x21 {
        return None;
    }
    match (stored, reply[2], reply[3]) {
        (_, 0xFF, 0xAA) => Some(Err(())),
        (false, 0xFF, 0xFF) => Some(Ok(())),
        (true, 0x02, 0x00) => Some(Ok(())),
        _ => None,
    }
}

/// Pixel buffer in B, G, R order, top row first.
pub fn solid(w: usize, h: usize, r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 3);
    for _ in 0..w * h {
        v.extend_from_slice(&[b, g, r]);
    }
    v
}

/// Convert RGBA pixels (e.g. from a canvas) to BGR.
pub fn rgba_to_bgr(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect()
}

pub fn brightness(level: u8) -> Packet {
    packet(&[0x12, 0x03, 0x00, 0x00, level.min(100)])
}

pub fn brightness_request() -> Packet {
    packet(&[0x12, 0x00, 0x00, 0x01])
}

/// `22 <enable> 00 00 <hh> <mm> <ss>`.
pub fn sleep_timer(enable: bool, secs: u32) -> Packet {
    let h = (secs / 3600).min(255) as u8;
    let m = ((secs / 60) % 60) as u8;
    let s = (secs % 60) as u8;
    packet(&[0x22, enable as u8, 0x00, 0x00, h, m, s])
}

pub fn show_logo(enable: bool) -> Packet {
    packet(&[0x23, enable as u8])
}

pub fn reset_all_pictures() -> Packet {
    packet(&[0x13, 0x61])
}

/// `21 01 00 00 <profile<<4 | key> … b14=FF`.
pub fn reset_key_picture(profile: u8, key: u8) -> Packet {
    let mut p = packet(&[0x21, 0x01, 0x00, 0x00, (profile << 4) | (key & 0x0F)]);
    p[14] = 0xFF;
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unhex;

    fn assert_pkt(p: &Packet, expect: &str) {
        let e = unhex(expect);
        assert_eq!(&p[..e.len()], &e[..]);
        assert!(p[e.len()..].iter().all(|&b| b == 0));
    }

    #[test]
    fn dp1_dp2() {
        assert_pkt(&full_icon_header(0, None).unwrap(), "21 00 00 00 00 3d 00 00 65 65");
        let blocks = bulk_blocks(&vec![1u8; ICON_BYTES]);
        assert_eq!(blocks.len(), 31);
        assert!(blocks[30][..492].iter().all(|&b| b == 1));
        assert!(blocks[30][492..].iter().all(|&b| b == 0));
        let f = Frame { profile: 2, count: 1, index: 0, interval_ms: 0 };
        assert_pkt(&full_icon_header(5, Some(f)).unwrap(), "21 01 00 00 25 3d 00 00 65 65 01 00 00 00");
    }

    #[test]
    fn dp3_dp4() {
        let mut px = solid(ICON_SIZE, ICON_SIZE, 0, 0, 0);
        px[3..6].copy_from_slice(&rgba_to_bgr(&[255, 0, 0, 255]));
        assert_eq!(&px[3..6], &[0, 0, 0xff]);
        assert_pkt(&panel_header(PANEL_BYTES, 0, 0, 799, 239), "21 00 00 01 65 04 00 00 00 00 1f 03 ef 00");
        let blocks = bulk_blocks(&vec![9u8; PANEL_BYTES]);
        assert_eq!(blocks.len(), 563);
        assert!(blocks[562][512..].iter().all(|&b| b == 0));
    }

    #[test]
    fn completions() {
        let live = crate::packet(&[0x21, 0x00, 0xFF, 0xFF]);
        assert_eq!(completion(&live, false), Some(Ok(())));
        assert_eq!(completion(&live, true), None);
        let stored = crate::packet(&[0x21, 0x01, 0x02, 0x00]);
        assert_eq!(completion(&stored, true), Some(Ok(())));
        assert_eq!(completion(&crate::packet(&[0xFF, 0xAA]), true), Some(Err(())));
    }
}
