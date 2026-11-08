//! Exact integer solid rings and complete single-side or uniform-ring dash/dot patterns.
//! Different side colours need diagonal joins
//! and remain unsupported instead of using rectangular corner guesses.
use crate::computed_style::{BorderColor, BorderStyle, Computed};
use crate::display_list::{DisplayList, Error, Rect, Rgb, Source};

/// CANDIDATE (FL41, ext only): `ext` admits DIFFERENT side colours when the nonzero sides are only an opposite pair (top+bottom
/// or left+right, or one side): no two painted sides meet at a corner, so no diagonal join exists. A transparent side in that
/// shape paints nothing (the background shows through, as in the shared-colour case). Non-ext callers keep refusing.
pub(crate) fn opposite_only(widths: [u32; 4]) -> bool {
    // order: top, right, bottom, left; adjacent pairs share a corner
    let nz = |i: usize| widths[i] > 0;
    !((nz(0) && nz(1)) || (nz(1) && nz(2)) || (nz(2) && nz(3)) || (nz(3) && nz(0)))
}

pub(crate) fn supported(style: Computed, ext: bool) -> bool {
    let widths = style.decor.used_border();
    let nonzero = widths.iter().filter(|&&width| width > 0).count();
    let patterned =
        (0..4).any(|side| widths[side] > 0 && style.decor.border_style[side] != BorderStyle::Solid);
    // CANDIDATE (FL45, ext only): an OPPOSITE pair of painted sides (no shared corner, so no join) may carry independent styles
    // (solid/dashed/dotted); each side is then painted alone by `paint`, with the same exact-fit pattern rule as a single side.
    let opposite_pair = ext && nonzero == 2 && opposite_only(widths);
    if patterned && nonzero != 1 && !opposite_pair {
        // Only a uniform four-sided ring has proven joins. Complete endpoint
        // dashes cover each square corner; overlap is the same opaque colour.
        if nonzero != 4
            || widths.iter().any(|&w| w != widths[0])
            || style
                .decor
                .border_style
                .iter()
                .any(|&kind| kind != style.decor.border_style[0])
        {
            return false;
        }
    }
    let mut color = None;
    style
        .decor
        .used_border()
        .iter()
        .enumerate()
        .all(|(side, &width)| {
            if width == 0 {
                return true;
            }
            let next = match style.decor.border_color[side] {
                BorderColor::Transparent => None,
                BorderColor::CurrentColor => Some(style.color),
                BorderColor::Opaque(rgb) => Some(rgb),
            };
            // Include transparent sides in the join check: mixing transparent
            // and opaque would require preserving each side's diagonal wedge.
            if ext && opposite_only(style.decor.used_border()) {
                return true;
            }
            if let Some(previous) = color {
                previous == next
            } else {
                color = Some(next);
                true
            }
        })
}

/// `content` is proven content geometry. padding_* already includes borders.
/// The outer ring is partitioned into nonoverlapping strips of one colour;
/// unequal widths and zero-width sides are exact, with no double painting.
pub(crate) fn paint(
    list: &mut DisplayList,
    content: Rect,
    style: Computed,
    source: Source,
    ext: bool,
) -> Result<(), Error> {
    if !supported(style, ext) {
        return Err(Error::InvalidSurface);
    }
    let widths = style.decor.used_border();
    let color = widths.iter().enumerate().find_map(|(side, &width)| {
        if width == 0 {
            return None;
        }
        match style.decor.border_color[side] {
            BorderColor::Transparent => None,
            BorderColor::CurrentColor => Some(style.color),
            BorderColor::Opaque(rgb) => Some(rgb),
        }
    });
    let Some(color) = color else {
        return Ok(());
    };
    let rgb = Rgb(
        u8::try_from((color >> 16) & 255).unwrap_or(0),
        u8::try_from((color >> 8) & 255).unwrap_or(0),
        u8::try_from(color & 255).unwrap_or(0),
    );
    let x = i64::from(content.x) - i64::from(style.padding_left);
    let y = i64::from(content.y) - i64::from(style.padding_top);
    let width = content
        .width
        .checked_add(style.padding_left)
        .and_then(|n| n.checked_add(style.padding_right))
        .ok_or(Error::InvalidSurface)?;
    let height = content
        .height
        .checked_add(style.padding_top)
        .and_then(|n| n.checked_add(style.padding_bottom))
        .ok_or(Error::InvalidSurface)?;
    if ext && widths.iter().filter(|&&w| w > 0).count() == 2 && opposite_only(widths) {
        // FL45: opposite pair, each side alone with its own style and colour.
        for side in 0..4 {
            let thickness = widths[side];
            if thickness == 0 {
                continue;
            }
            let c = match style.decor.border_color[side] {
                BorderColor::Transparent => continue,
                BorderColor::CurrentColor => style.color,
                BorderColor::Opaque(rgb) => rgb,
            };
            let rgb = Rgb(u8::try_from((c >> 16) & 255).unwrap_or(0), u8::try_from((c >> 8) & 255).unwrap_or(0), u8::try_from(c & 255).unwrap_or(0));
            let kind = style.decor.border_style[side];
            if kind == BorderStyle::Solid {
                let (rx, ry, rw, rh) = match side {
                    0 => (x, y, width, thickness),
                    2 => (x, y + i64::from(height) - i64::from(thickness), width, thickness),
                    3 => (x, y, thickness, height),
                    _ => (x + i64::from(width) - i64::from(thickness), y, thickness, height),
                };
                if rw == 0 || rh == 0 {
                    continue;
                }
                list.fill(
                    Rect {
                        x: i32::try_from(rx).map_err(|_| Error::InvalidSurface)?,
                        y: i32::try_from(ry).map_err(|_| Error::InvalidSurface)?,
                        width: rw,
                        height: rh,
                    },
                    rgb,
                    source,
                )?;
            } else {
                paint_pattern(list, x, y, width, height, side, thickness, kind, rgb, source)?;
            }
        }
        return Ok(());
    }
    if let Some(side) =
        (0..4).find(|&s| widths[s] > 0 && style.decor.border_style[s] != BorderStyle::Solid)
    {
        let kind = style.decor.border_style[side];
        let thickness = widths[side];
        if widths.iter().all(|&w| w > 0) {
            // Both fits are proven before a side is painted. The same opaque
            // colour covers each complete square corner without a join wedge.
            pattern(width, thickness, kind).ok_or(Error::InvalidSurface)?;
            pattern(height, thickness, kind).ok_or(Error::InvalidSurface)?;
            for side in 0..4 {
                paint_pattern(
                    list, x, y, width, height, side, thickness, kind, rgb, source,
                )?;
            }
            return Ok(());
        }
        return paint_pattern(
            list, x, y, width, height, side, thickness, kind, rgb, source,
        );
    }
    let [top, right, bottom, left] = widths;
    let side_rgb = |side: usize| -> Option<Rgb> {
        let c = match style.decor.border_color[side] {
            BorderColor::Transparent => return None,
            BorderColor::CurrentColor => style.color,
            BorderColor::Opaque(rgb) => rgb,
        };
        Some(Rgb(u8::try_from((c >> 16) & 255).unwrap_or(0), u8::try_from((c >> 8) & 255).unwrap_or(0), u8::try_from(c & 255).unwrap_or(0)))
    };
    let middle = height
        .checked_sub(top)
        .and_then(|n| n.checked_sub(bottom))
        .ok_or(Error::InvalidSurface)?;
    let rects = [
        (x, y, width, top),
        (x, y + i64::from(height) - i64::from(bottom), width, bottom),
        (x, y + i64::from(top), left, middle),
        (
            x + i64::from(width) - i64::from(right),
            y + i64::from(top),
            right,
            middle,
        ),
    ];
    for (side, (x, y, width, height)) in [0usize, 2, 3, 1].into_iter().zip(rects) {
        if width == 0 || height == 0 {
            continue;
        }
        // Shared colour (the proven case) uses the common `rgb`; the ext opposite-pair case paints each side in its own colour.
        let rgb = if ext && opposite_only(widths) {
            match side_rgb(side) {
                Some(c) => c,
                None => continue,
            }
        } else {
            rgb
        };
        list.fill(
            Rect {
                x: i32::try_from(x).map_err(|_| Error::InvalidSurface)?,
                y: i32::try_from(y).map_err(|_| Error::InvalidSurface)?,
                width,
                height,
            },
            rgb,
            source,
        )?;
    }
    Ok(())
}

/// A complete symmetric integer pattern, without short end dashes, circular
/// coverage, fitted fractional gaps or joins. No alternate rounding policy.
fn pattern(length: u32, thickness: u32, style: BorderStyle) -> Option<(u32, u32)> {
    let (dash, gap) = match style {
        BorderStyle::Dotted if thickness <= 3 => (thickness, thickness),
        BorderStyle::Dashed if thickness < 3 => {
            (thickness.checked_mul(3)?, thickness.checked_mul(2)?)
        }
        BorderStyle::Dashed => (thickness.checked_mul(2)?, thickness),
        _ => return None,
    };
    let period = dash.checked_add(gap)?;
    let span = length.checked_add(gap)?;
    if period == 0 || span % period != 0 || span / period < 2 {
        return None;
    }
    Some((dash, gap))
}

#[allow(clippy::too_many_arguments)] // Proven box geometry and one side's pattern.
fn paint_pattern(
    list: &mut DisplayList,
    x: i64,
    y: i64,
    width: u32,
    height: u32,
    side: usize,
    thickness: u32,
    style: BorderStyle,
    color: Rgb,
    source: Source,
) -> Result<(), Error> {
    let horizontal = side == 0 || side == 2;
    let length = if horizontal { width } else { height };
    let (dash, gap) = pattern(length, thickness, style).ok_or(Error::InvalidSurface)?;
    let count = length.checked_add(gap).ok_or(Error::InvalidSurface)? / (dash + gap);
    for index in 0..count {
        let offset = i64::from(index) * i64::from(dash + gap);
        let (dx, dy, w, h) = match side {
            0 => (offset, 0, dash, thickness),
            1 => (
                i64::from(width) - i64::from(thickness),
                offset,
                thickness,
                dash,
            ),
            2 => (
                offset,
                i64::from(height) - i64::from(thickness),
                dash,
                thickness,
            ),
            _ => (0, offset, thickness, dash),
        };
        list.fill(
            Rect {
                x: i32::try_from(x + dx).map_err(|_| Error::InvalidSurface)?,
                y: i32::try_from(y + dy).map_err(|_| Error::InvalidSurface)?,
                width: w,
                height: h,
            },
            color,
            source,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_edges_refuse_without_returning_a_page() {
        let mut style = Computed::INITIAL;
        style.decor.border_style = [crate::computed_style::BorderStyle::Solid; 4];
        style.decor.border_width = [2; 4];
        style.padding_left = u32::MAX;
        style.padding_right = 2;
        let mut list = DisplayList::default();
        assert_eq!(
            paint(
                &mut list,
                Rect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10
                },
                style,
                Source::default(),
                false
            ),
            Err(Error::InvalidSurface)
        );
        assert!(list.commands().is_empty());
        style.padding_left = 2;
        style.padding_top = 2;
        style.padding_bottom = 2;
        assert_eq!(
            paint(
                &mut list,
                Rect {
                    x: i32::MIN,
                    y: 0,
                    width: 10,
                    height: 10
                },
                style,
                Source::default(),
                false
            ),
            Err(Error::InvalidSurface)
        );
        assert!(list.commands().is_empty());
    }
}
