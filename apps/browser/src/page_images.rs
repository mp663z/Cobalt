//! Bounded native data-image decoding for Page view. No network requests.
use kobo_web_document::{basic_page::PartialStats, box_tree::BoxTree};
use std::collections::BTreeMap;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_PIXELS: u64 = 1_048_576;

pub(super) fn prepare(
    tree: &BoxTree,
    stats: &mut PartialStats,
) -> BTreeMap<
    usize,
    (
        kobo_web_document::display_list::Rgb,
        Option<kobo_image::Picture>,
    ),
> {
    let mut images = BTreeMap::new();
    let mut pixels_left = MAX_PIXELS;
    let mut bytes_left = MAX_BYTES;
    let mut index = 0;
    for node in &tree.boxes {
        if node.kind != kobo_web_document::box_tree::BoxKind::Image {
            continue;
        }
        let (red, green, blue) = kobo_web_document::basic_page::image_sentinel(index);
        index += 1;
        let marker = kobo_web_document::display_list::Rgb(red, green, blue);
        let result = (|| {
            let source = node.source?;
            let uri = node.image.as_ref()?.src_data_uri.as_deref()?;
            let bytes = decode_uri(uri, bytes_left)?;
            bytes_left = bytes_left.checked_sub(bytes.len())?;
            let (w, h) = kobo_image::size(&bytes).ok()?;
            let pixels = u64::from(w).checked_mul(u64::from(h))?;
            if pixels == 0 || pixels > pixels_left {
                return None;
            }
            let picture = kobo_image::decode_colour(&bytes).ok()?;
            pixels_left -= pixels;
            Some((source, picture))
        })();
        if let Some((source, picture)) = result {
            images.insert(source, (marker, Some(picture)));
            *stats
                .approximations
                .entry("native-image-marker-geometry")
                .or_default() += 1;
        } else {
            if let Some(source) = node.source {
                images.insert(source, (marker, None));
            }
            *stats.skips.entry("native-image-unavailable").or_default() += 1;
        }
    }
    images
}

fn decode_uri(uri: &str, budget: usize) -> Option<Vec<u8>> {
    let (header, payload) = uri.split_once(',')?;
    if !["data:image/png;base64", "data:image/jpeg;base64"]
        .iter()
        .any(|h| header.eq_ignore_ascii_case(h))
    {
        return None;
    }
    let bytes = payload.as_bytes();
    if bytes.len() % 4 != 0 || bytes.len() / 4 * 3 > budget.saturating_add(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let count = bytes.len() / 4;
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let pad = usize::from(chunk[3] == b'=') + usize::from(chunk[2] == b'=');
        if (pad > 0 && i + 1 != count) || (chunk[2] == b'=' && chunk[3] != b'=') {
            return None;
        }
        let mut n = 0_u32;
        for (j, &b) in chunk.iter().enumerate() {
            let value = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if j >= 4 - pad => 0,
                _ => return None,
            };
            n = (n << 6) | u32::from(value);
        }
        if (pad == 2 && n & 0xffff != 0) || (pad == 1 && n & 0xff != 0) {
            return None;
        }
        out.push(n.to_be_bytes()[1]);
        if pad < 2 {
            out.push(n.to_be_bytes()[2]);
        }
        if pad < 1 {
            out.push(n.to_be_bytes()[3]);
        }
    }
    (out.len() <= budget).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_base64_and_resource_budgets() {
        assert_eq!(
            decode_uri("data:image/png;base64,AQID", 3),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            decode_uri("data:image/jpeg;base64,AQI=", 2),
            Some(vec![1, 2])
        );
        assert_eq!(decode_uri("data:image/png;base64,AQ==", 1), Some(vec![1]));
        for uri in [
            "data:image/svg+xml;base64,AQID",
            "data:image/png,AQID",
            "data:image/png;base64,AQ=Z",
            "data:image/png;base64,AQ==AQ==",
            "data:image/png;base64,AR==",
            "data:image/png;base64,AQJ=",
            "data:image/png;base64, AQID",
            "data:image/png;base64,AQ",
        ] {
            assert!(decode_uri(uri, 10).is_none(), "{uri}");
        }
        assert!(decode_uri("data:image/png;base64,AQID", 2).is_none());
    }

    #[test]
    fn canonical_padding_bits_are_checked_exhaustively() {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for byte in 0..=255_u32 {
            for low in 0..16 {
                let uri = format!(
                    "data:image/png;base64,{}{}==",
                    char::from(A[(byte >> 2) as usize]),
                    char::from(A[((byte & 3) * 16 + low) as usize])
                );
                let decoded = decode_uri(&uri, 1);
                if low == 0 {
                    assert_eq!(decoded, Some(vec![byte as u8]));
                } else {
                    assert!(decoded.is_none());
                }
            }
        }
        for pair in 0..=65535_u32 {
            for low in 0..4 {
                let uri = format!(
                    "data:image/jpeg;base64,{}{}{}=",
                    char::from(A[(pair >> 10) as usize]),
                    char::from(A[((pair >> 4) & 63) as usize]),
                    char::from(A[((pair & 15) * 4 + low) as usize])
                );
                let decoded = decode_uri(&uri, 2);
                if low == 0 {
                    assert_eq!(decoded, Some(vec![(pair >> 8) as u8, pair as u8]));
                } else {
                    assert!(decoded.is_none());
                }
            }
        }
    }
}
