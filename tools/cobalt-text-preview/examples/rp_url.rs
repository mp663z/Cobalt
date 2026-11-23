//! Research-only (private): render a locally fetched live page (html + its
//! linked stylesheets in document order) through the BASIC partial painter
//! with strip replay. The supply handshake requires sheets.len() == claimed
//! host sheet links, so the fetch pipeline must deliver every linked sheet.
use cobalt_text_preview::LocalFace;
use kobo_web_document::basic_page::{normalize_for_basic, paint_basic_page};
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::display_list::{Command, DisplayList, Rect, Rgb};
use kobo_web_document::{parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget};

const WIDTH: u32 = 800;
const STRIP_H: u32 = 512;

fn shifted(cmd: &Command, dy: i64, strip_h: u32) -> Option<Command> {
    let sh = i64::from(strip_h);
    match cmd {
        Command::Fill { rect, color, source } => {
            let y = i64::from(rect.y) + dy;
            if y + i64::from(rect.height) <= 0 || y >= sh {
                return None;
            }
            let y = i32::try_from(y).ok()?;
            Some(Command::Fill { rect: Rect { x: rect.x, y, width: rect.width, height: rect.height }, color: *color, source: *source })
        }
        Command::GlyphRun { bounds, layout, coverage, color, source } => {
            let y = i64::from(bounds.y) + dy;
            if y + i64::from(bounds.height) <= 0 || y >= sh {
                return None;
            }
            let by = i32::try_from(y).ok()?;
            let ly = i32::try_from(i64::from(layout.y) + dy).ok()?;
            Some(Command::GlyphRun {
                bounds: Rect { x: bounds.x, y: by, width: bounds.width, height: bounds.height },
                layout: Rect { x: layout.x, y: ly, width: layout.width, height: layout.height },
                coverage: coverage.clone(),
                color: *color,
                source: *source,
            })
        }
        Command::PushClip(rect) => {
            let y = i32::try_from(i64::from(rect.y) + dy).ok()?;
            Some(Command::PushClip(Rect { x: rect.x, y, width: rect.width, height: rect.height }))
        }
        Command::PopClip => Some(Command::PopClip),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let html_path = &a[1];
    let sheet_dir = &a[2];
    let out_path = &a[3];
    let bytes = normalize_for_basic(&std::fs::read(html_path).unwrap());
    let mut sheets: Vec<Vec<u8>> = Vec::new();
    for i in 0..64 {
        match std::fs::read(format!("{sheet_dir}/sheet-{i}.css")) {
            Ok(s) => sheets.push(s),
            Err(_) => break,
        }
    }
    let budget = SupplyBudget { max_linked_sheets: 48, max_sheet_bytes: 2 * 1024 * 1024, max_aggregate_bytes: 16 * 1024 * 1024 };
    let limits = Limits { max_input_bytes: 8 * 1024 * 1024, ..Limits::DEFAULT };
    let media = MediaContext::new(WIDTH).unwrap();
    let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &sheets, &limits, Some(media), budget);
    let bt = BoxTree::from_style(&st);
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    let (_, _, h1) = paint_basic_page(&bt, WIDTH, 1, &face).unwrap();
    let height = u32::try_from(h1.clamp(1, 100_000)).unwrap();
    let (cmds, stats, _) = paint_basic_page(&bt, WIDTH, height, &face).unwrap();
    let mut rgba = vec![255u8; WIDTH as usize * height as usize * 4];
    let mut top = 0i64;
    let mut strip_overflows = 0u32;
    while top < i64::from(height) {
        let sh = STRIP_H.min(height - top as u32);
        let mut list = DisplayList::default();
        for c in &cmds {
            if let Some(c2) = shifted(c, -top, sh) {
                let r = match c2 {
                    Command::Fill { rect, color, source } => list.fill(rect, color, source),
                    Command::GlyphRun { bounds, layout, coverage, color, source } => {
                        list.glyph_run(bounds, layout, coverage, color, source)
                    }
                    Command::PushClip(rect) => list.push_clip(rect),
                    Command::PopClip => list.pop_clip(),
                };
                if r.is_err() {
                    strip_overflows += 1;
                    break;
                }
            }
        }
        let img = list.rasterize(WIDTH, sh, Rgb(255, 255, 255)).unwrap();
        let off = top as usize * WIDTH as usize * 4;
        rgba[off..off + img.len()].copy_from_slice(&img);
        top += i64::from(sh);
    }
    std::fs::write(out_path, &rgba).unwrap();
    println!("{html_path}: sheets={} {WIDTH}x{height} complete={} output_truncated={} intake_truncated={} strip_overflows={} blocks={} text_blocks={} glyph_runs={} fills={}", sheets.len(), stats.complete(), stats.output_truncated, stats.intake_truncated, strip_overflows, stats.blocks_painted, stats.text_blocks_painted, stats.glyph_runs, stats.fills);
    println!("  skips: {:?}", stats.skips);
    println!("  approximations: {:?}", stats.approximations);
}
