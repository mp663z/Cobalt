//! CLI validation regression for `--color-scheme`: the flag is consumed only on
//! the masked render paths, so scheme-without-`--blocks` must be REJECTED, never
//! silently ignored. An accepted `dark` must carry the operator-visible
//! no-dark-matching note; `light` on a dark-gated page paints; absent keeps the
//! legacy refusal. Spawns the real binary (actual integration, not a seam).
use std::process::Command;

const FONT: &str = "../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf";
const GATED: &str = "<!doctype html><html><head><link rel=\"stylesheet\" media=\"(prefers-color-scheme: dark)\" href=\"data:text/css;base64,cCB7IGRpc3BsYXk6IGdyaWQgfQ==\"></head><body><p style=\"font-size:16px\">Hello world</p></body></html>";
const PLAIN: &str = "<!doctype html><html><body><p style=\"font-size:16px\">Hello world</p></body></html>";

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cobalt-text-preview"))
        .args(args)
        .output()
        .expect("binary runs")
}

#[test]
fn scheme_without_blocks_is_rejected_not_ignored() {
    let dir = std::env::temp_dir();
    let html = dir.join("cli-scheme-gated.html");
    let out = dir.join("cli-scheme-noblocks.ppm");
    std::fs::write(&html, GATED).unwrap();
    let output = run(&[
        html.to_str().unwrap(),
        out.to_str().unwrap(),
        "200",
        "80",
        FONT,
        "--color-scheme",
        "light",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--color-scheme requires --blocks"),
        "validation must name the rejected combination, got: {stderr}"
    );
}

#[test]
fn dark_is_accepted_with_operator_warning_and_light_paints() {
    let dir = std::env::temp_dir();
    let html = dir.join("cli-scheme-gated2.html");
    std::fs::write(&html, GATED).unwrap();
    // dark: accepted, warns, and refuses the gated page exactly like absent.
    let dark_out = dir.join("cli-scheme-dark.ppm");
    let dark = run(&[
        html.to_str().unwrap(),
        dark_out.to_str().unwrap(),
        "200",
        "80",
        FONT,
        "--blocks",
        "--static-capture",
        "--color-scheme",
        "dark",
    ]);
    let dark_stderr = String::from_utf8_lossy(&dark.stderr);
    assert!(
        dark_stderr.contains("no dark matching exists"),
        "accepted dark must carry the operator-visible no-matching note, got: {dark_stderr}"
    );
    assert!(!dark.status.success(), "dark keeps Unknown: gated page still refuses");
    // absent: gated page refuses (legacy absent-context behavior).
    let absent_out = dir.join("cli-scheme-absent.ppm");
    let absent = run(&[
        html.to_str().unwrap(),
        absent_out.to_str().unwrap(),
        "200",
        "80",
        FONT,
        "--blocks",
        "--static-capture",
    ]);
    assert!(!absent.status.success());
    // light: actual light assertion skips the dark-gated sheet; page paints
    // byte-identical to the ungated page.
    let light_out = dir.join("cli-scheme-light.ppm");
    let light = run(&[
        html.to_str().unwrap(),
        light_out.to_str().unwrap(),
        "200",
        "80",
        FONT,
        "--blocks",
        "--static-capture",
        "--color-scheme",
        "light",
    ]);
    assert!(light.status.success(), "light paints: {}", String::from_utf8_lossy(&light.stderr));
    let plain_html = dir.join("cli-scheme-plain.html");
    let plain_out = dir.join("cli-scheme-plain.ppm");
    std::fs::write(&plain_html, PLAIN).unwrap();
    assert!(run(&[
        plain_html.to_str().unwrap(),
        plain_out.to_str().unwrap(),
        "200",
        "80",
        FONT,
        "--blocks",
        "--static-capture",
    ])
    .status
    .success());
    assert_eq!(
        std::fs::read(&light_out).unwrap(),
        std::fs::read(&plain_out).unwrap(),
        "skipped dark-gated sheet leaves no trace in the painted output"
    );
}
