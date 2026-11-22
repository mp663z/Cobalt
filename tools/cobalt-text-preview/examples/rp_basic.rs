//! Research-only (private): render a frozen page through the BASIC partial
//! painter and write raw RGBA + a census line. The painter emits an
//! unbounded command vec; this harness replays it into per-strip bounded
//! display lists (gate budgets untouched) and blits the strips into one
//! full-height image. Never a shipping path; the census must travel with
//! the image.
use cobalt_text_preview::LocalFace;
use kobo_web_document::basic_page::paint_basic_page;
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
    let page = &a[1];
    let home = std::env::var("HOME").unwrap();
    let bytes = std::fs::read(format!("{home}/cb/rr74/rp/{page}.html")).unwrap();
    let budget = SupplyBudget { max_linked_sheets: 48, max_sheet_bytes: 2 * 1024 * 1024, max_aggregate_bytes: 16 * 1024 * 1024 };
    let limits = Limits { max_input_bytes: 8 * 1024 * 1024, ..Limits::DEFAULT };
    let media = MediaContext::new(WIDTH).unwrap();
    let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &[], &limits, Some(media), budget);
    let bt = BoxTree::from_style(&st);
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    // Pass 1: measure content height (canvas fill covers 1px; discarded).
    let (_, _, h1) = paint_basic_page(&bt, WIDTH, 1, &face).unwrap();
    if h1 > 100_000 {
        println!("{page}: WARNING raw content height {h1} clamped to 100000");
    }
    let height = u32::try_from(h1.clamp(1, 100_000)).unwrap();
    // Pass 2: paint with the canvas covering the full content height.
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
    std::fs::write(format!("/tmp/basic-{page}.rgba"), &rgba).unwrap();
    println!("{page}: {WIDTH}x{height} complete={} output_truncated={} intake_truncated={} strip_overflows={} blocks={} text_blocks={} glyph_runs={} fills={}", stats.complete(), stats.output_truncated, stats.intake_truncated, strip_overflows, stats.blocks_painted, stats.text_blocks_painted, stats.glyph_runs, stats.fills);
    println!("  skips: {:?}", stats.skips);
    println!("  approximations: {:?}", stats.approximations);
    println!("  bytes={}", rgba.len());
}
