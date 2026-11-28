//! Bounded native image decoding for Page view. Data images come from the page itself; external images arrive as
//! already-fetched bytes (see `image_supply`). This module never touches the network.
use kobo_web_document::{basic_page::PartialStats, box_tree::BoxTree};
use std::collections::BTreeMap;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_PIXELS: u64 = 1_048_576;

/// Fetched external image bodies keyed by the `src` exactly as the page authored it.
pub(crate) type External = BTreeMap<String, Vec<u8>>;

/// Gives every external `img` whose body was fetched its pixel size, which is what makes it an image box. Bodies the
/// decoder cannot size are left out, so those images keep their alt text.
pub(crate) fn size_external(tree: &mut kobo_web_document::style_tree::StyleTree, external: &External) {
    if external.is_empty() {
        return;
    }
    for node in &mut tree.nodes {
        let Some(facts) = node.image.as_mut() else {
            continue;
        };
        if facts.src_data_uri.is_some() {
            continue;
        }
        let Some(body) = facts.src_external.as_ref().and_then(|src| external.get(src)) else {
            continue;
        };
        if let Ok((w, h)) = kobo_image::size(body) {
            if w > 0 && h > 0 {
                facts.external_dims = Some((w, h));
            }
        }
    }
}

pub(super) fn prepare(
    tree: &BoxTree,
    stats: &mut PartialStats,
    external: &External,
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
            let facts = node.image.as_ref()?;
            let bytes: std::borrow::Cow<'_, [u8]> = match facts.src_data_uri.as_deref() {
                Some(uri) => decode_uri(uri, bytes_left)?.into(),
                None => {
                    let body = external.get(facts.src_external.as_deref()?)?;
                    if body.len() > bytes_left {
                        return None;
                    }
                    body.as_slice().into()
                }
            };
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

/// Page-view-only fit. Keep the engine's offline marker contract unchanged.
/// Width comes from the actual first layout, so a narrow parent is respected.
/// One re-layout is enough for ordinary blocks; this remains counted basic
/// marker geometry, not a replaced-element CSS conformance claim.
pub(super) fn fit_geometry(
    tree: &mut BoxTree,
    commands: &[kobo_web_document::display_list::Command],
    images: &BTreeMap<
        usize,
        (
            kobo_web_document::display_list::Rgb,
            Option<kobo_image::Picture>,
        ),
    >,
) -> bool {
    let mut changed = false;
    for index in 0..tree.boxes.len() {
        let node = &tree.boxes[index];
        if node.kind != kobo_web_document::box_tree::BoxKind::Image {
            continue;
        }
        let Some(source) = node.source else {
            continue;
        };
        let Some((marker, Some(image))) = images.get(&source) else {
            continue;
        };
        let width = commands.iter().find_map(|command| match command {
            kobo_web_document::display_list::Command::Fill {
                rect,
                color,
                source: origin,
            } if origin.node == Some(source) && color == marker => Some(rect.width),
            _ => None,
        });
        // Zero-area fills are omitted by the painter. A directly containing
        // normal block with an explicit zero content width still tells us
        // enough to collapse the successful image without touching fallback.
        let width = width.or_else(|| {
            let parent = node.parent.and_then(|p| tree.boxes.get(p))?;
            (parent.kind == kobo_web_document::box_tree::BoxKind::Block
                && parent.style.width == kobo_web_document::computed_style::Length::Px(0))
            .then_some(0)
        });
        let Some(width) = width else {
            continue;
        };
        let (_, height) = kobo_image::fitted_size(
            (image.width(), image.height()),
            width.min(image.width()),
            4096,
        );
        if node.style.padding_top != height || !node.children.is_empty() {
            let node = &mut tree.boxes[index];
            node.style.padding_top = height;
            // Successful decode replaces alt content in layout as well as paint.
            // The detached text box remains inert in the arena; failed images
            // are untouched and keep their readable fallback.
            node.children.clear();
            changed = true;
        }
    }
    changed
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
