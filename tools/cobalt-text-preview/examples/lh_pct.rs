//! FL73 constructed: line-height percent x font-size -> whole px or ComputedValue refusal (E1 default parse). Synthetic.
use kobo_web_document::*;
fn main() {
    println!("font-size line-height  result   (E1 parse_style_tree_with_media; nodes refused counted via ComputedValue records)");
    for fs in [12u32, 13, 14, 15, 16, 17, 18, 20, 24] {
        for lh in ["100%", "120%", "125%", "130%", "150%", "1.2", "1.3", "20px"] {
            let html = format!("<!doctype html><style>body{{font-size:{fs}px;line-height:{lh}}}</style><body><p>Hello <b>w</b></p></body>");
            let t = parse_style_tree_with_media(html.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(800).unwrap());
            let n = t.diagnostics.count(style_diagnostics::RefusalReason::ComputedValue);
            println!("{fs:>9}px {lh:>8}  {}", if t.unsupported { format!("REFUSE ComputedValue x{n}") } else { "keep".to_string() });
        }
    }
}
