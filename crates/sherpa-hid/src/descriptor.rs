//! Minimal HID report-descriptor parser: finds top-level collections, their usage
//! page/usage, the report ids they use and the size of each report.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collection {
    pub usage_page: u16,
    pub usage: u16,
    /// Report ids used by the collection (empty when the descriptor declares none).
    pub report_ids: Vec<u8>,
    /// Report payload size in bytes (excluding the id byte), by kind and id.
    pub reports: BTreeMap<(ReportKind, u8), usize>,
}

impl Collection {
    pub fn uses_report_ids(&self) -> bool {
        !self.report_ids.is_empty()
    }

    /// First report id of a kind, or 0 when ids are not used.
    pub fn report_id(&self, kind: ReportKind) -> u8 {
        self.reports.keys().find(|(k, _)| *k == kind).map(|(_, id)| *id).unwrap_or(0)
    }

    pub fn has_report(&self, kind: ReportKind, id: u8) -> bool {
        self.reports.contains_key(&(kind, id))
    }

    pub fn len(&self, kind: ReportKind) -> usize {
        self.reports.iter().filter(|((k, _), _)| *k == kind).map(|(_, v)| *v).max().unwrap_or(0)
    }
}

#[derive(Clone, Copy, Default)]
struct Globals {
    usage_page: u16,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

/// Parse a report descriptor into its top-level collections.
pub fn parse(desc: &[u8]) -> Vec<Collection> {
    let mut out: Vec<Collection> = Vec::new();
    let mut bits: Vec<BTreeMap<(ReportKind, u8), u32>> = Vec::new();
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    let mut usages: Vec<u32> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == 0xFE {
            // Long item: size in next byte.
            let size = *desc.get(i + 1).unwrap_or(&0) as usize;
            i += 3 + size;
            continue;
        }
        let size = match prefix & 3 {
            3 => 4,
            n => n as usize,
        };
        let typ = (prefix >> 2) & 3;
        let tag = prefix >> 4;
        let mut data = 0u32;
        for b in 0..size {
            data |= (*desc.get(i + 1 + b).unwrap_or(&0) as u32) << (8 * b);
        }
        i += 1 + size;
        match typ {
            0 => {
                // Main
                let kind = match tag {
                    0x8 => Some(ReportKind::Input),
                    0x9 => Some(ReportKind::Output),
                    0xB => Some(ReportKind::Feature),
                    _ => None,
                };
                if let Some(kind) = kind {
                    if let Some(map) = bits.last_mut() {
                        *map.entry((kind, g.report_id)).or_insert(0) += g.report_size * g.report_count;
                    }
                    if let Some(c) = out.last_mut() {
                        if g.report_id != 0 && !c.report_ids.contains(&g.report_id) {
                            c.report_ids.push(g.report_id);
                        }
                    }
                } else if tag == 0xA {
                    if depth == 0 {
                        let (page, usage) = match usages.first() {
                            Some(&u) if u > 0xFFFF => ((u >> 16) as u16, u as u16),
                            Some(&u) => (g.usage_page, u as u16),
                            None => (g.usage_page, 0),
                        };
                        out.push(Collection { usage_page: page, usage, ..Default::default() });
                        bits.push(BTreeMap::new());
                    }
                    depth += 1;
                } else if tag == 0xC {
                    depth = depth.saturating_sub(1);
                }
                usages.clear();
            }
            1 => match tag {
                0x0 => g.usage_page = data as u16,
                0x7 => g.report_size = data,
                0x8 => g.report_id = data as u8,
                0x9 => g.report_count = data,
                0xA => stack.push(g),
                0xB => g = stack.pop().unwrap_or(g),
                _ => {}
            },
            2 if tag == 0x0 => {
                usages.push(if size == 4 { data } else { data & 0xFFFF });
            }
            _ => {}
        }
    }
    for (c, b) in out.iter_mut().zip(bits) {
        c.reports = b.into_iter().map(|(k, v)| (k, (v as usize).div_ceil(8))).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_and_keyboard() {
        // Keyboard collection (no report ids) followed by a vendor collection with id 4.
        let desc: Vec<u8> = vec![
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, // keyboard
            0x75, 0x08, 0x95, 0x08, 0x81, 0x02, 0xC0, //
            0x06, 0x00, 0xFF, 0x09, 0x01, 0xA1, 0x01, // vendor FF00:01
            0x85, 0x04, 0x75, 0x08, 0x95, 0x40, 0x09, 0x01, 0x81, 0x02, 0x09, 0x01, 0x91, 0x02, //
            0xA1, 0x02, 0x95, 0x10, 0xB1, 0x02, 0xC0, 0xC0,
        ];
        let c = parse(&desc);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].usage_page, c[0].usage, c[0].uses_report_ids()), (1, 6, false));
        assert_eq!(c[0].len(ReportKind::Input), 8);
        assert_eq!((c[1].usage_page, c[1].usage), (0xFF00, 1));
        assert_eq!(c[1].report_ids, vec![4]);
        assert_eq!(c[1].report_id(ReportKind::Output), 4);
        assert_eq!(c[1].len(ReportKind::Output), 64);
        assert_eq!(c[1].len(ReportKind::Feature), 16);
    }
}
