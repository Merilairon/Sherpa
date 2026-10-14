//! Launch content: a payload stored on the keyboard against a key (spec 04 §7.5, 05 §2–3).

use crate::types::LaunchKind;

/// Encode `<kind byte> ‖ UTF-8 text`.
pub fn encode(kind: LaunchKind, text: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(text.len() + 1);
    v.push(kind as u8);
    v.extend_from_slice(text.as_bytes());
    v
}

/// Join arguments with the `~` separator, dropping trailing empty ones.
pub fn join_args<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut v: Vec<&str> = parts.into_iter().collect();
    while v.len() > 1 && v.last().is_some_and(|s| s.is_empty()) {
        v.pop();
    }
    v.join("~")
}

/// A decoded launch payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub kind: u8,
    pub text: String,
}

impl Launch {
    pub fn kind(&self) -> Option<LaunchKind> {
        LaunchKind::from_u8(self.kind)
    }

    /// `~`-separated arguments.
    pub fn args(&self) -> Vec<&str> {
        self.text.split('~').collect()
    }
}

/// Decode a payload read back from the device: drop zero bytes, then `kind = data[0]`
/// and `text = UTF-8(data[1:])`, trimmed (spec 05 §2.2 step 6).
pub fn decode(data: &[u8]) -> Option<Launch> {
    let bytes: Vec<u8> = data.iter().copied().filter(|&b| b != 0).collect();
    let (&kind, rest) = bytes.split_first()?;
    Some(Launch { kind, text: String::from_utf8_lossy(rest).trim().to_string() })
}

/// One chunk of a launch-content write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub seq: u8,
    pub data: &'a [u8],
}

/// Split a payload into ≤60-byte chunks with the exact `seq` rule of spec 04 §7.5:
/// total ≤ 60 → 0; otherwise the chunk at offset 0 gets 1, and every later chunk gets 3
/// if `total < offset + 60`, else 2.
pub fn chunks(payload: &[u8]) -> Vec<Chunk<'_>> {
    chunks_sized(payload, 60)
}

pub fn chunks_sized(payload: &[u8], size: usize) -> Vec<Chunk<'_>> {
    let total = payload.len();
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < total || (total == 0 && out.is_empty()) {
        let end = (offset + size).min(total);
        let seq = if total <= size {
            0
        } else if offset == 0 {
            1
        } else if total < offset + size {
            3
        } else {
            2
        };
        out.push(Chunk { seq, data: &payload[offset..end] });
        if total == 0 {
            break;
        }
        offset = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_rule() {
        let p130 = vec![1u8; 130];
        let c: Vec<_> = chunks(&p130).iter().map(|c| (c.data.len(), c.seq)).collect();
        assert_eq!(c, vec![(60, 1), (60, 2), (10, 3)]);
        let p120 = vec![1u8; 120];
        let c: Vec<_> = chunks(&p120).iter().map(|c| (c.data.len(), c.seq)).collect();
        assert_eq!(c, vec![(60, 1), (60, 2)]);
        let p20 = vec![1u8; 20];
        assert_eq!(chunks(&p20)[0].seq, 0);
        let p60 = vec![1u8; 60];
        assert_eq!(chunks(&p60).len(), 1);
    }

    #[test]
    fn decode_r9() {
        let l = decode(&crate::unhex("02 68 74 74 70 3a 2f 2f 61 00 00")).unwrap();
        assert_eq!(l.kind(), Some(LaunchKind::Url));
        assert_eq!(l.text, "http://a");
        assert_eq!(join_args(["Set Current Scene", "Live", ""]), "Set Current Scene~Live");
    }
}
