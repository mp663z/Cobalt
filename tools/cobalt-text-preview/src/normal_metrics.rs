//! Byte-aware, restricted normal metrics. Not a generic platform font policy.
use kobo_web_document::css_text_page::NormalLineMetrics;
#[derive(Clone, Copy)]
pub(crate) struct Metrics {
    units: u16,
    ascent: i16,
    descent: i16,
    gap: i16,
}
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn table(bytes: &[u8], tag: [u8; 4]) -> Option<&[u8]> {
    let count = usize::from(u16_at(bytes, 4)?);
    if count > 256 {
        return None;
    }
    let mut found = None;
    for i in 0..count {
        let record = 12 + i * 16;
        let t = bytes.get(record..record + 16)?;
        if t.get(..4)? == tag {
            if found.is_some() {
                return None;
            }
            let offset = usize::try_from(u32::from_be_bytes(t[8..12].try_into().ok()?)).ok()?;
            let length = usize::try_from(u32::from_be_bytes(t[12..16].try_into().ok()?)).ok()?;
            found = Some(bytes.get(offset..offset.checked_add(length)?)?);
        }
    }
    found
}
impl Metrics {
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        // Single TrueType face only; no collection/OTTO/variation/platform overrides.
        if bytes.get(..4)? != [0, 1, 0, 0] {
            return None;
        }
        let count = usize::from(u16_at(bytes, 4)?);
        if count == 0 || count > 256 {
            return None;
        }
        let directory_end = 12_usize.checked_add(count.checked_mul(16)?)?;
        bytes.get(..directory_end)?;
        let mut tags = Vec::new();
        let mut ranges = Vec::new();
        for index in 0..count {
            let record = bytes.get(12 + index * 16..12 + (index + 1) * 16)?;
            let tag: [u8; 4] = record[..4].try_into().ok()?;
            if tags.contains(&tag) || tag == *b"VDMX" || tag == *b"fvar" {
                return None;
            }
            tags.push(tag);
            let offset =
                usize::try_from(u32::from_be_bytes(record[8..12].try_into().ok()?)).ok()?;
            let length =
                usize::try_from(u32::from_be_bytes(record[12..16].try_into().ok()?)).ok()?;
            if offset < directory_end {
                return None;
            }
            let end = offset.checked_add(length)?;
            bytes.get(offset..end)?;
            if length > 0 && ranges.iter().any(|&(a, b)| offset < b && a < end) {
                return None;
            }
            ranges.push((offset, end));
        }
        if table(bytes, *b"VDMX").is_some() || table(bytes, *b"fvar").is_some() {
            return None;
        }
        let os = table(bytes, *b"OS/2")?;
        let os_min = match u16_at(os, 0)? {
            0 => 78,
            1 => 86,
            2..=4 => 96,
            5 => 100,
            _ => return None,
        };
        if os.len() < os_min || u16_at(os, 62)? & 128 != 0 {
            return None;
        }
        let head = table(bytes, *b"head")?;
        let h = table(bytes, *b"hhea")?;
        if head.len() < 54
            || h.len() < 36
            || head.get(..4)? != [0, 1, 0, 0]
            || head.get(12..16)? != [0x5f, 0x0f, 0x3c, 0xf5]
            || h.get(..4)? != [0, 1, 0, 0]
        {
            return None;
        }
        let units = u16_at(head, 18)?;
        let ascent = i16::from_be_bytes(h.get(4..6)?.try_into().ok()?);
        let descent = i16::from_be_bytes(h.get(6..8)?.try_into().ok()?);
        let gap = i16::from_be_bytes(h.get(8..10)?.try_into().ok()?);
        if !(16..=16384).contains(&units) || ascent <= 0 || descent > 0 || gap < 0 {
            return None;
        }
        Some(Self {
            units,
            ascent,
            descent,
            gap,
        })
    }
    pub(crate) fn at(self, size: u32) -> Option<NormalLineMetrics> {
        // Only the exact sampled integer sizes activate. Every other size uses legacy.
        if ![8, 10, 12, 13, 14, 15, 16, 17, 18, 20, 24, 32, 48].contains(&size) {
            return None;
        }
        let round = |value: i32| -> Option<u32> {
            let numerator = u32::try_from(value).ok()?.checked_mul(size)?;
            numerator
                .checked_add(u32::from(self.units) / 2)
                .map(|n| n / u32::from(self.units))
        };
        let a = round(i32::from(self.ascent))?;
        let d = round(-i32::from(self.descent))?;
        let g = round(i32::from(self.gap))?;
        let height = a.checked_add(d)?.checked_add(g)?;
        let baseline = i32::try_from(a.checked_add(g / 2)?).ok()?;
        (height > 0 && baseline >= 0 && u32::try_from(baseline).ok()? < height)
            .then_some(NormalLineMetrics { height, baseline })
    }
}
pub(crate) struct Pair {
    pub regular: Option<Metrics>,
    pub bold: Option<Metrics>,
    pub has_bold: bool,
}
impl Pair {
    pub(crate) fn at(&self, size: u32, bold: bool) -> Option<NormalLineMetrics> {
        let regular = self.regular?.at(size)?;
        if self.has_bold && self.bold?.at(size)? != regular {
            return None;
        }
        if bold && !self.has_bold {
            return None;
        }
        Some(regular)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(typo: bool, vdmx: bool) -> Vec<u8> {
        let count = if vdmx { 4_u16 } else { 3 };
        let start = 12 + usize::from(count) * 16;
        let mut bytes = vec![0; start + 256];
        bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
        bytes[4..6].copy_from_slice(&count.to_be_bytes());
        for (i, (tag, offset, length)) in [
            (b"head", start, 54),
            (b"hhea", start + 54, 36),
            (b"OS/2", start + 90, 78),
        ]
        .iter()
        .enumerate()
        {
            let r = 12 + i * 16;
            bytes[r..r + 4].copy_from_slice(*tag);
            bytes[r + 8..r + 12].copy_from_slice(&u32::try_from(*offset).unwrap().to_be_bytes());
            bytes[r + 12..r + 16].copy_from_slice(&u32::try_from(*length).unwrap().to_be_bytes());
        }
        if vdmx {
            let r = 60;
            bytes[r..r + 4].copy_from_slice(b"VDMX");
            bytes[r + 8..r + 12]
                .copy_from_slice(&u32::try_from(start + 200).unwrap().to_be_bytes());
        }
        bytes[start..start + 4].copy_from_slice(&[0, 1, 0, 0]);
        bytes[start + 12..start + 16].copy_from_slice(&[0x5f, 0x0f, 0x3c, 0xf5]);
        bytes[start + 54..start + 58].copy_from_slice(&[0, 1, 0, 0]);
        bytes[start + 18..start + 20].copy_from_slice(&2048_u16.to_be_bytes());
        for (off, n) in [(58, 1825_i16), (60, -443), (62, 87)] {
            bytes[start + off..start + off + 2].copy_from_slice(&n.to_be_bytes());
        }
        if typo {
            bytes[start + 152..start + 154].copy_from_slice(&128_u16.to_be_bytes());
        }
        bytes
    }
    #[test]
    fn bounded_metadata_and_components_are_checked() {
        let bytes = source(false, false);
        let m = Metrics::parse(&bytes).unwrap();
        assert_eq!(
            m.at(16),
            Some(NormalLineMetrics {
                height: 18,
                baseline: 14
            })
        );
        assert_eq!(
            m.at(12),
            Some(NormalLineMetrics {
                height: 15,
                baseline: 11
            })
        );
        assert!(m.at(7).is_none());
        assert!(m.at(49).is_none());
        for size in [0, 7, 9, 11, 19, 21, 23, 25, 31, 33, 47, 49, u32::MAX] {
            let pair = Pair {
                regular: Some(m),
                bold: Some(m),
                has_bold: true,
            };
            assert!(pair.at(size, false).is_none());
            assert!(pair.at(size, true).is_none());
        }
        let mut overlap = bytes.clone();
        overlap[36..40].copy_from_slice(&60_u32.to_be_bytes());
        assert!(Metrics::parse(&overlap).is_none());
        let mut version = bytes.clone();
        version[60] = 2;
        assert!(Metrics::parse(&version).is_none());
        assert!(Metrics::parse(&source(true, false)).is_none());
        assert!(Metrics::parse(&source(false, true)).is_none());
        let mut bad = bytes.clone();
        bad[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(Metrics::parse(&bad).is_none());
        assert!(Metrics::parse(&bytes[..20]).is_none());
    }
    #[test]
    fn paired_eligibility_and_disagreement_are_all_or_nothing() {
        let a = Metrics::parse(&source(false, false)).unwrap();
        let mut b = a;
        b.ascent += 256;
        for pair in [
            Pair {
                regular: Some(a),
                bold: None,
                has_bold: true,
            },
            Pair {
                regular: Some(a),
                bold: Some(b),
                has_bold: true,
            },
            Pair {
                regular: None,
                bold: Some(a),
                has_bold: true,
            },
        ] {
            assert!(pair.at(16, false).is_none());
            assert!(pair.at(16, true).is_none());
        }
        let pair = Pair {
            regular: Some(a),
            bold: Some(a),
            has_bold: true,
        };
        assert_eq!(pair.at(16, false), pair.at(16, true));
    }
}
