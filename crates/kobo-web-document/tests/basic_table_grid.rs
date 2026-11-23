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
            top: -i32::try_from(s).unwrap(),
            width: s / 2,
            height: s,
            coverage: vec![255; (s * s / 2) as usize],
        })
    }
}

#[test]
fn columns_align_across_rows() {
    let html = br"<!doctype html><style>body{margin:0}.grid{display:table;width:200px}.row{display:table-row}.cell{display:table-cell;background:#123456}</style><table class=grid><tr class=row><td class=cell>A</td><td class=cell>BBBB</td></tr><tr class=row><td class=cell>AAAAAA</td><td class=cell>B</td></tr></table>";
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
fn unsupported_cell_span_uses_counted_content_preserving_fallback() {
    let html=br"<!doctype html><style>table{display:table}tbody{display:table-row-group}tr{display:table-row}td{display:table-cell;background:#123456}</style><table><tr><td rowspan=2>A</td></tr></table>";
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
    assert_eq!(stats.approximations.get("table-span-not-wired"), Some(&1));
    assert_eq!(stats.approximations.get("table-stacked-fallback"), Some(&1));
    assert_eq!(stats.text_blocks_painted, 1);
    assert!(commands.iter().any(|command| matches!(
        command,
        Command::Fill {
            color: Rgb(0x12, 0x34, 0x56),
            ..
        }
    )));
}

#[test]
fn direct_cells_receive_an_anonymous_row() {
    use kobo_web_document::computed_style::Display;
    let html = br"<!doctype html><grid><cell>A</cell> <cell>B</cell></grid>";
    let mut style = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    for node in &mut style.nodes {
        node.style.display = match node.tag.as_str() {
            "grid" => Display::Table,
            "cell" => Display::TableCell,
            _ => node.style.display,
        };
    }
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert_eq!(stats.text_blocks_painted, 2);
    assert!(!commands.is_empty());
}

#[test]
fn cell_backgrounds_fill_the_shared_row_height_and_padding_is_reserved() {
    use kobo_web_document::computed_style::Display;
    let html = br"<!doctype html><style>body{margin:0}td{padding:4px;background:#123456}</style><table><tr><td>A</td><td>A much longer cell that wraps</td></tr></table>";
    let mut style = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    for node in &mut style.nodes {
        node.style.display = match node.tag.as_str() {
            "table" => Display::Table,
            "tbody" => Display::TableRowGroup,
            "tr" => Display::TableRow,
            "td" => Display::TableCell,
            _ => node.style.display,
        };
    }
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 110, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    let cells: Vec<_> = commands
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
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].y, cells[1].y);
    assert_eq!(cells[0].height, cells[1].height);
    assert!(cells[0].height > 28, "second cell must wrap");
}

#[test]
fn page_intake_retains_native_tables_and_stacks_unsupported_spans() {
    use kobo_web_document::basic_page::normalize_for_basic_full;
    let ((bytes, _), _, _) = normalize_for_basic_full(
        br"<!doctype html><table><tr><td rowspan=2>A</td></tr></table><p>After</p>",
    );
    assert!(String::from_utf8_lossy(&bytes).contains("<table>"));
    let style = kobo_web_document::parse_basic_style_tree(
        &bytes,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        None,
    );
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert_eq!(stats.approximations.get("table-stacked-fallback"), Some(&1));
    assert_eq!(stats.text_blocks_painted, 2);
    assert!(!commands.is_empty());
}

#[test]
fn native_page_table_has_shared_columns_without_model_overrides() {
    let ((bytes, _), _, _) = kobo_web_document::basic_page::normalize_for_basic_full(
        br"<!doctype html><style>body{margin:0}table{width:200px}td{padding:3px;background:#123456}</style><table><tr><td>A</td><td>BBBBB</td></tr><tr><td>AAAAAA</td><td>B</td></tr></table><p>After</p>",
    );
    let style = kobo_web_document::parse_basic_style_tree(
        &bytes,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        None,
    );
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert!(!stats.approximations.contains_key("table-stacked-fallback"));
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
    assert_eq!(stats.text_blocks_painted, 5);
}

#[test]
fn page_table_ua_defaults_do_not_override_authored_display_none() {
    let style = kobo_web_document::parse_basic_style_tree(
        br#"<!doctype html><table style="display:none"><tr><td>Hidden</td></tr></table><p>After</p>"#,
        &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL, None,
    );
    let boxes = BoxTree::from_style(&style);
    assert!(!boxes
        .boxes
        .iter()
        .any(|node| node.text.as_deref() == Some("Hidden")));
    let (_, stats, _) = paint_basic_page(&boxes, 300, 500, &Face).unwrap();
    assert_eq!(stats.text_blocks_painted, 1);
}

#[test]
fn nested_block_cell_falls_back_without_losing_text() {
    let html = br"<!doctype html><table><caption>Caption</caption><tr><td><p>Block cell</p></td><td>Peer</td></tr></table><p>After</p>";
    let style = kobo_web_document::parse_basic_style_tree(
        html,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        None,
    );
    let (_, stats, _) = paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert_eq!(stats.approximations.get("table-stacked-fallback"), Some(&1));
    assert_eq!(stats.text_blocks_painted, 4);
}

#[test]
fn separated_solid_cell_borders_use_the_shared_row_border_box() {
    let style = kobo_web_document::parse_basic_style_tree(
        br"<!doctype html><style>body{margin:0}td{padding:4px;border:2px solid #456789;background:#123456}</style><table><tr><td>A</td><td>Longer content wraps over several lines</td></tr></table>",
        &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL, None,
    );
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 160, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert!(!stats.approximations.contains_key("table-stacked-fallback"));
    let edges: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Fill {
                rect,
                color: Rgb(0x45, 0x67, 0x89),
                ..
            } => Some(rect),
            _ => None,
        })
        .collect();
    assert_eq!(edges.len(), 8);
    assert_eq!(edges[0].height, 2);
    assert_eq!(edges[1].width, 2);
    assert_eq!(edges[1].height, edges[5].height);
    assert!(edges[1].height > 28);
}

#[test]
fn native_outer_table_border_surrounds_grid_without_stacking() {
    let style = kobo_web_document::parse_basic_style_tree(
        br"<!doctype html><style>body{margin:0}table{width:180px;border:3px solid #456789;background:#eeeeee}td{padding:4px}</style><table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table>",
        &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL, None,
    );
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert!(!stats.approximations.contains_key("table-stacked-fallback"));
    let edges: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Fill {
                rect,
                color: Rgb(0x45, 0x67, 0x89),
                ..
            } => Some(rect),
            _ => None,
        })
        .collect();
    assert_eq!(edges.len(), 4);
    assert_eq!(edges[0].width, 180);
    assert_eq!(edges[0].height, 3);
    assert_eq!(edges[1].x, edges[0].x + 177);
    assert!(edges[1].height > 60);
    assert_eq!(stats.text_blocks_painted, 4);
}

#[test]
fn short_rows_leave_empty_slots_without_stacking_or_shifting_columns() {
    let style = kobo_web_document::parse_basic_style_tree(
        br"<!doctype html><style>body{margin:0}table{width:220px}td{padding:4px;background:#123456}</style><table><tr><td>Short row</td></tr><tr><td>A</td><td>B</td><td>C</td></tr><tr><td>D</td><td>E</td></tr></table>",
        &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL, None,
    );
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert!(!stats.approximations.contains_key("table-stacked-fallback"));
    let cells: Vec<_> = commands
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
    assert_eq!(cells.len(), 6);
    assert_eq!(cells[0].x, cells[1].x);
    assert_eq!(cells[1].x, cells[4].x);
    assert_eq!(cells[2].x, cells[5].x);
    assert_eq!(cells[1].width, cells[4].width);
    assert!(cells[3].x > cells[2].x);
    assert_eq!(stats.text_blocks_painted, 6);
}

#[test]
fn stacked_table_fallback_backgrounds_stay_inside_outer_border() {
    let style = kobo_web_document::parse_basic_style_tree(
        br"<!doctype html><style>body{margin:0}table{width:180px;border:3px solid #456789}td{padding:4px;border:2px solid #666666;background:#123456}</style><table><tr><td rowspan=2>A</td></tr><tr><td>B</td><td>C</td></tr></table>",
        &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL, None,
    );
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert_eq!(stats.approximations.get("table-stacked-fallback"), Some(&1));
    let fills: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Fill { rect, color, .. } => Some((rect, color)),
            _ => None,
        })
        .collect();
    let outer = fills
        .iter()
        .find(|(_, c)| **c == Rgb(0x45, 0x67, 0x89))
        .unwrap()
        .0;
    assert_eq!(outer.width, 180);
    for (rect, color) in fills {
        if *color == Rgb(0x12, 0x34, 0x56) {
            assert!(rect.x >= outer.x + 3);
            assert!(rect.x + i32::try_from(rect.width).unwrap() <= outer.x + 177);
        }
    }
    assert_eq!(stats.text_blocks_painted, 3);
}

#[test]
fn colspan_cell_covers_shared_columns_and_internal_gap() {
    let style = kobo_web_document::parse_basic_style_tree(
        br"<!doctype html><style>body{margin:0}table{width:220px}td{padding:4px;background:#123456}</style><table><tr><td colspan=2>Spanning heading</td></tr><tr><td>A</td><td>B</td></tr></table>",
        &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL, None,
    );
    let (commands, stats, _) =
        paint_basic_page(&BoxTree::from_style(&style), 300, 500, &Face).unwrap();
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert!(!stats.approximations.contains_key("table-stacked-fallback"));
    let cells: Vec<_> = commands
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
    assert_eq!(cells.len(), 3);
    assert_eq!(cells[0].x, cells[1].x);
    assert_eq!(cells[0].width, cells[1].width + 2 + cells[2].width);
    assert_eq!(stats.text_blocks_painted, 3);
}
