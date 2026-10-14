//! Pure, I/O-free building blocks for talking to Mountain peripherals.
//!
//! Everything in this crate is derived from the cleanroom specification in
//! `docs/cleanroom/`. Packet builders return complete 64-byte payloads (without the
//! HID report-ID byte); parsers take payloads with the report ID already stripped.

pub mod binding;
pub mod bitmap;
pub mod displaypad;
pub mod everest;
pub mod everest60;
pub mod keys;
pub mod launch;
pub mod macros;
pub mod makalu67;
pub mod model;
pub mod multipart;
pub mod shortcut;
pub mod tables;
pub mod types;

/// Mountain's USB vendor id.
pub const VENDOR_ID: u16 = 0x3282;

/// One 64-byte HID payload (report ID not included).
pub type Packet = [u8; 64];

/// Error type for protocol-level validation failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoError(pub String);

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProtoError {}

pub type ProtoResult<T> = Result<T, ProtoError>;

pub(crate) fn err<T>(msg: impl Into<String>) -> ProtoResult<T> {
    Err(ProtoError(msg.into()))
}

/// Helper: a zero-filled packet starting with the given bytes.
pub fn packet(prefix: &[u8]) -> Packet {
    let mut p = [0u8; 64];
    p[..prefix.len()].copy_from_slice(prefix);
    p
}

/// Format bytes as lowercase hex separated by spaces (used in logs and errors).
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// Parse a hex string such as `"14 2c 00"` (whitespace optional). Test helper.
pub fn unhex(s: &str) -> Vec<u8> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (0..clean.len() / 2).map(|i| u8::from_str_radix(&clean[2 * i..2 * i + 2], 16).unwrap()).collect()
}
