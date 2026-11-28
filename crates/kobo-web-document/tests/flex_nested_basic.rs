//! Nested flex rows in the basic page painter (Sphinx/mobile-nav slice).
//!
//! A flex container whose child is itself a flex container keeps the row:
//! the nested child is measured by content, grows into a share with
//! `flex:auto`, and shrinkable measured items shrink to fit (CSS Flexbox 1
//! sec 9.7). Whitespace-only text children are anonymous flex items that
//! are not rendered (sec 4), so they neither disqualify the row nor take a
//! slot. Pure-inline inline children (a nav-logo link) blockify as items
//! measured by content. Only an unshrinkable nested row wider than its
//! container still stacks. No Chrome cross-check on this box: geometry is
//! pinned directly.
use kobo_web_document::{
    Limits, SupplyBudget,
    basic_page::{PartialStats, normalize_for_basic, paint_basic_page},
    box_tree::BoxTree,
    css_text_page::FontProvider,
    display_list::{Command, Rect, Rgb},
    inline_lines::GlyphBitmap,
};

struct Face;
impl FontProvider for Face {
    fn has_glyph(&self, c: char, _: bool) -> Option<bool> {
        Some(c.is_ascii())
    }
    fn advance(&self, _: char, s: u32, _: bool) -> Option<u32> {
        Some(s / 2)
    }
    fn line_height(&self, s: u32, _: bool) -> Option<u32> {
        Some(s + 4)
    }
    fn baseline_offset(&self, s: u32, _: bool) -> Option<i32> {
        i32::try_from(s).ok()
    }
    fn raster(&self, c: char, s: u32, _: bool) -> Option<GlyphBitmap> {
        c.is_ascii().then(|| GlyphBitmap {
            left: 0,
            top: -i32::try_from(s).unwrap(),
            width: s / 2,
            height: s,
            coverage: vec![255; (s * s / 2) as usize],
        })
    }
}

fn paint(body: &str) -> (Vec<Command>, PartialStats, i64) {
    let html = format!("<!doctype html><style>body{{margin:0}}</style>{body}");
    let bytes = normalize_for_basic(html.as_bytes());
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        &bytes,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    paint_basic_page(&BoxTree::from_style(&styled), 800, 600, &Face).unwrap()
}

fn fills(commands: &[Command], color: u32) -> Vec<Rect> {
    let [r, g, b] = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Fill {
                rect,
                color: Rgb(cr, cg, cb),
                ..
            } if (*cr, *cg, *cb) == (r, g, b) => Some(*rect),
            _ => None,
        })
        .collect()
}

/// The Sphinx mobile-bar shape: an outer flex row whose items are a
/// fixed-width label and a nested flex wrapper holding a content-measured
/// logo block and a nested flex search form. Every item paints on the
/// same line, in order, instead of stacking.
#[test]
fn nested_rows_share_one_line() {
    let (commands, stats, _) = paint(
        r#"<div style="display:flex">
  <div style="display:flex;width:40px;height:40px;background:#111111"><span style="display:block;width:10px;height:10px;background:#555555"></span></div>
  <div style="display:flex;width:400px;height:40px;background:#eeeeee">
    <div style="background:#222222">logo</div>
    <form style="display:flex;width:100px;background:#333333"><span style="display:block;width:20px;height:20px;background:#444444"></span></form>
  </div>
</div>"#,
    );
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert_eq!(
        stats.approximations.get("flex-as-block"),
        None,
        "{:?}",
        stats.approximations
    );
    assert_eq!(
        stats.approximations.get("flex-row-refused-nested-flex"),
        None,
        "{:?}",
        stats.approximations
    );
    assert!(
        stats
            .approximations
            .get("flex-row-basic")
            .copied()
            .unwrap_or(0)
            >= 3,
        "{:?}",
        stats.approximations
    );
    let label = fills(&commands, 0x111111);
    let logo = fills(&commands, 0x222222);
    let form = fills(&commands, 0x333333);
    assert_eq!(label.len(), 1, "{label:?}");
    assert_eq!(logo.len(), 1, "{logo:?}");
    assert_eq!(form.len(), 1, "{form:?}");
    assert_eq!((label[0].x, label[0].y), (0, 0));
    // "logo" is 4 chars x 8px + 8 slack = a 40px content-measured slot
    // right after the 40px label; the form follows it. All one line.
    assert_eq!((logo[0].x, logo[0].y), (40, 0));
    assert_eq!((form[0].x, form[0].y), (80, 0));
}

/// Whitespace-only text children of a flex container are anonymous items
/// that are not rendered: they take no slot and do not stack the row.
#[test]
fn whitespace_children_take_no_slot() {
    let (commands, stats, _) = paint(
        "<div style=\"display:flex\">\n  <div style=\"width:100px;background:#9cf\"><div style=\"height:20px\"></div></div>\n  <div style=\"width:100px;background:#fc9\"><div style=\"height:20px\"></div></div>\n</div>",
    );
    assert_eq!(
        stats.approximations.get("flex-as-block"),
        None,
        "{:?}",
        stats.approximations
    );
    let a = fills(&commands, 0x99ccff);
    let b = fills(&commands, 0xffcc99);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(b.len(), 1, "{b:?}");
    assert_eq!((a[0].x, a[0].y, a[0].width), (0, 0, 100));
    assert_eq!((b[0].x, b[0].y, b[0].width), (100, 0, 100));
}

/// A pure-inline inline child (a link) blockifies as a flex item measured
/// by content instead of disqualifying the row.
#[test]
fn pure_inline_child_measured_by_content() {
    let (commands, stats, _) = paint(
        "<div style=\"display:flex\"><a>Home</a><div style=\"width:80px;background:#fc9\"><div style=\"height:20px\"></div></div></div>",
    );
    assert_eq!(
        stats.approximations.get("flex-as-block"),
        None,
        "{:?}",
        stats.approximations
    );
    assert_eq!(stats.approximations.get("flex-row-natural-width"), Some(&1));
    // "Home" is 4 chars x 8px + 8 slack = a 40px slot; the sibling paints
    // right after it on the same line (stacked would be x=0, y>=20).
    let b = fills(&commands, 0xffcc99);
    assert_eq!(b.len(), 1, "{b:?}");
    assert_eq!((b[0].x, b[0].y), (40, 0));
}

/// Measured items wider than the container shrink in proportion to base
/// size x flex-shrink instead of overflowing or stacking. The strict
/// all-block flex path declines this row (an inline item is present), so
/// the basic painter's own shrink pass does the work.
#[test]
fn shrinkable_items_shrink_to_fit() {
    let (_, stats, _) = paint(
        r#"<div style="display:flex;width:160px"><a>looooooooooooooong</a><div>alsoloooooooooooooooooong</div></div>"#,
    );
    assert_eq!(
        stats.approximations.get("flex-as-block"),
        None,
        "{:?}",
        stats.approximations
    );
    assert_eq!(
        stats.approximations.get("flex-row-shrunk"),
        Some(&1),
        "{:?}",
        stats.approximations
    );
    assert_eq!(stats.approximations.get("flex-row-basic"), Some(&1));
    assert_eq!(stats.approximations.get("flex-row-refused-overflow"), None);
}

/// flex-shrink:0 items never shrink; a row that still overflows falls back
/// to equal shares rather than painting past the border box.
#[test]
fn unshrinkable_items_keep_the_overflow_fallback() {
    let (_, stats, _) = paint(
        r#"<div style="display:flex;width:160px"><a style="flex-shrink:0">looooooooooooooong</a><div style="flex-shrink:0">alsoloooooooooooooooooong</div></div>"#,
    );
    assert_eq!(
        stats.approximations.get("flex-row-shrunk"),
        None,
        "{:?}",
        stats.approximations
    );
    assert_eq!(
        stats.approximations.get("flex-row-equal-fallback"),
        Some(&1),
        "{:?}",
        stats.approximations
    );
    assert_eq!(stats.approximations.get("flex-row-basic"), Some(&1));
}

/// An unshrinkable nested flex row wider than its container is the one
/// case that still stacks: painting it would spill past the border box.
#[test]
fn unshrinkable_nested_overflow_still_stacks() {
    let long = "l".repeat(80); // 80 chars x 8px + 8 = 648px > the 120px container
    let (_, stats, _) = paint(&format!(
        "<div style=\"display:flex;width:120px\"><div style=\"display:flex;flex-shrink:0\"><span>{long}</span></div><div style=\"width:50px\"><div style=\"height:20px\"></div></div></div>"
    ));
    assert_eq!(
        stats.approximations.get("flex-row-refused-nested-flex"),
        Some(&1),
        "{:?}",
        stats.approximations
    );
    // The outer row stacks on the refusal, and the nested container's own
    // pure-inline content is not row-eligible either.
    assert_eq!(
        stats.approximations.get("flex-as-block"),
        Some(&2),
        "{:?}",
        stats.approximations
    );
}

/// An empty element whose only paint is a background (the Sphinx
/// hamburger span, `background:currentColor`) is a visible flex item: it
/// takes a slot and its background paints, instead of the row falling
/// back to a block stack where the item vanishes. A textless share child
/// does not trip the 64px narrow-column refusal, which guards readable
/// text only.
#[test]
fn decorated_empty_item_paints() {
    let (commands, stats, _) = paint(
        r#"<label style="width:40px;display:flex;padding:8px;color:#112233"><span style="height:2px;width:100%;background:currentColor"></span></label>"#,
    );
    assert_eq!(
        stats.approximations.get("flex-as-block"),
        None,
        "{:?}",
        stats.approximations
    );
    assert_eq!(
        stats.approximations.get("flex-row-refused-narrow"),
        None,
        "{:?}",
        stats.approximations
    );
    let bar = fills(&commands, 0x112233);
    assert_eq!(bar.len(), 1, "{bar:?}");
    assert_eq!(bar[0].height, 2, "{bar:?}");
    assert!(bar[0].width > 0, "{bar:?}");
}

/// A growing pure-inline child (`flex:1`, like mdBook's .menu-title)
/// takes a share of the leftover row width instead of a content-measured
/// fixed slot, so its text-align:center centres across the row.
#[test]
fn growing_inline_child_takes_share() {
    let (commands, stats, _) = paint(
        r#"<div style="display:flex"><div style="width:100px;background:#9cf"><div style="height:20px"></div></div><h1 style="flex:1;text-align:center;font-size:16px;margin:0">Title</h1></div>"#,
    );
    assert_eq!(
        stats.approximations.get("flex-row-inline-grow"),
        Some(&1),
        "{:?}",
        stats.approximations
    );
    // "Title" is 5 chars x 8px = 40px, centred in a 700px share that
    // starts at x=100: the run begins near x=430, nowhere near the
    // left-aligned x=100 a content-width slot would give.
    let run_start = commands
        .iter()
        .filter_map(|command| match command {
            Command::GlyphRun { layout, .. } => Some(layout.x),
            _ => None,
        })
        .min()
        .expect("a glyph run");
    assert!(run_start > 300, "run starts at {run_start}");
}

/// align-items:stretch (the initial value): an auto-height item with a
/// background paints that background across the full row height; an item
/// with a definite px height keeps its own size. Borders are not
/// stretched (disclosed approximation).
#[test]
fn stretch_extends_item_backgrounds() {
    let (commands, stats, _) = paint(
        r#"<div style="display:flex"><div style="width:60px;height:40px;background:#123456"></div><div style="width:60px;background:#abcdef"><span style="display:block;height:10px"></span></div><div style="width:60px;height:20px;background:#fedcba"></div></div>"#,
    );
    assert_eq!(
        stats.approximations.get("flex-row-stretch-bg"),
        Some(&1),
        "{:?}",
        stats.approximations
    );
    let stretched = fills(&commands, 0xabcdef);
    assert_eq!(stretched.len(), 1, "{stretched:?}");
    assert_eq!(stretched[0].height, 40, "{stretched:?}");
    let definite = fills(&commands, 0xfedcba);
    assert_eq!(definite.len(), 1, "{definite:?}");
    assert_eq!(definite[0].height, 20, "{definite:?}");
}

/// align-items:center shifts an item down by half the row slack instead
/// of top-aligning it.
#[test]
fn center_shifts_items_vertically() {
    let (commands, stats, _) = paint(
        r#"<div style="display:flex;align-items:center"><div style="width:60px;height:40px;background:#123456"></div><div style="width:60px;background:#abcdef"><span style="display:block;height:10px"></span></div></div>"#,
    );
    assert_eq!(
        stats.approximations.get("flex-row-item-centered"),
        Some(&1),
        "{:?}",
        stats.approximations
    );
    let centered = fills(&commands, 0xabcdef);
    assert_eq!(centered.len(), 1, "{centered:?}");
    assert_eq!(centered[0].y, 15, "{centered:?}");
}
