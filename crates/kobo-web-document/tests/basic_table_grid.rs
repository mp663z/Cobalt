//! Shared-column table paint, independent of HTML tag rewriting.
use kobo_web_document::{
    basic_page::paint_basic_page,
    box_tree::BoxTree,
    css_text_page::FontProvider,
    display_list::{Command, Rgb},
    inline_lines::GlyphBitmap,
    Limits, SupplyBudget,
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
            top: -(s as i32),
            width: s / 2,
            height: s,
            coverage: vec![255; (s * s / 2) as usize],
        })
    }
}

#[test]
fn columns_align_across_rows() {
    let html = br#"<!doctype html><style>body{margin:0}.grid{display:table;width:200px}.row{display:table-row}.cell{display:table-cell;background:#123456}</style><table class=grid><tr class=row><td class=cell>A</td><td class=cell>BBBB</td></tr><tr class=row><td class=cell>AAAAAA</td><td class=cell>B</td></tr></table>"#;
    let mut style = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    for node in &mut style.nodes {
        use kobo_web_document::computed_style::Display;
        node.style.display = match node.tag.as_str() {
            "table" => Display::Table,
            "tbody" => Display::TableRowGroup,
            "tr" => Display::TableRow,
            "td" => Display::TableCell,
            _ => node.style.display,
        };
    }
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    let fills: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Fill {
                rect,
                color: Rgb(0x12, 0x34, 0x56),
                ..
            } => Some(rect),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 4);
    assert_eq!(fills[0].x, fills[2].x);
    assert_eq!(fills[1].x, fills[3].x);
    assert_eq!(fills[0].width, fills[2].width);
    assert_eq!(fills[1].width, fills[3].width);
    assert!(fills[1].x > fills[0].x);
    assert!(fills[2].y > fills[0].y);
}

#[test]
fn unsupported_cell_span_is_counted_without_partial_ink() {
    let html=br#"<!doctype html><style>table{display:table}tbody{display:table-row-group}tr{display:table-row}td{display:table-cell;background:#123456}</style><table><tr><td colspan=2>A</td></tr></table>"#;
    let mut style = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    for node in &mut style.nodes {
        use kobo_web_document::computed_style::Display;
        node.style.display = match node.tag.as_str() {
            "table" => Display::Table,
            "tbody" => Display::TableRowGroup,
            "tr" => Display::TableRow,
            "td" => Display::TableCell,
            _ => node.style.display,
        };
    }
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert_eq!(stats.skips.get("table-span-not-wired"), Some(&1));
    assert!(!commands.iter().any(|command| matches!(
        command,
        Command::Fill {
            color: Rgb(0x12, 0x34, 0x56),
            ..
        }
    )));
}
