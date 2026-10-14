//! Reassembly of multi-part replies (spec 04 §9).

use crate::Packet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `11 01 00 00`: `remaining` at b6, payload b7..b63.
    A,
    /// `11 02 …`, `11 01 00 02/03`: `remaining` at b4, payload b5..b63.
    B,
    /// `11 01 00 05`: like B, but if b4 is `0A` or `FF`, `remaining` is at b5 and the
    /// payload starts at b6.
    B5,
}

impl Form {
    /// (remaining, payload slice) of one part.
    pub fn split(self, p: &Packet) -> (u8, &[u8]) {
        match self {
            Form::A => (p[6], &p[7..]),
            Form::B => (p[4], &p[5..]),
            Form::B5 => {
                if p[4] == 0x0A || p[4] == 0xFF {
                    (p[5], &p[6..])
                } else {
                    (p[4], &p[5..])
                }
            }
        }
    }
}

/// Collects the parts of one multi-part reply.
#[derive(Debug, Clone)]
pub struct Assembler {
    form: Form,
    total: Option<usize>,
    parts: Vec<Option<Vec<u8>>>,
}

impl Assembler {
    pub fn new(form: Form) -> Self {
        Assembler { form, total: None, parts: Vec::new() }
    }

    /// Feed one matching reply. Returns `true` once the part with `remaining = 0` arrived.
    pub fn push(&mut self, p: &Packet) -> bool {
        let (remaining, slice) = self.form.split(p);
        let remaining = remaining as usize;
        let total = *self.total.get_or_insert_with(|| remaining + 1);
        if self.parts.len() < total {
            self.parts.resize(total, None);
        }
        if remaining < total {
            let index = total - remaining - 1;
            self.parts[index] = Some(slice.to_vec());
        }
        remaining == 0
    }

    /// Concatenate the slices in index order (missing parts are skipped).
    pub fn finish(self) -> Vec<u8> {
        self.parts.into_iter().flatten().flatten().collect()
    }

    pub fn is_complete(&self) -> bool {
        self.total.is_some() && self.parts.iter().all(Option::is_some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet;

    fn part(remaining: u8, fill: u8) -> Packet {
        let mut p = packet(&[0x11, 0x02, 0x00, 0x01, remaining]);
        p[5..].fill(fill);
        p
    }

    #[test]
    fn r5_r6() {
        let mut a = Assembler::new(Form::B);
        assert!(!a.push(&part(2, 0xA0)));
        assert!(!a.push(&part(1, 0xA1)));
        assert!(a.push(&part(0, 0xA2)));
        assert!(a.is_complete());
        let v = a.finish();
        assert_eq!(v.len(), 59 * 3);
        assert!(v[..59].iter().all(|&b| b == 0xA0));
        assert!(v[59..118].iter().all(|&b| b == 0xA1));
        assert!(v[118..].iter().all(|&b| b == 0xA2));
    }

    #[test]
    fn form_a_and_b5() {
        let mut p = packet(&[0x11, 0x01, 0x00, 0x00, 5, 9, 0]);
        p[7] = 0xEE;
        let mut a = Assembler::new(Form::A);
        assert!(a.push(&p));
        assert_eq!(a.finish()[0], 0xEE);
        let q = packet(&[0x11, 0x01, 0x00, 0x05, 0x0A, 0x00, 0x77]);
        let (r, s) = Form::B5.split(&q);
        assert_eq!((r, s[0], s.len()), (0, 0x77, 58));
    }
}
