//! Shared baseline for the size probes: every binary links the same SDK
//! application, so the stripped size difference is the parser alone.

use kobo_sdk::{ActionId, Context, KoboApp, ScreenBuilder};

#[derive(Default)]
pub struct Baseline;

impl KoboApp for Baseline {
    fn on_start(&mut self, context: &mut Context) {
        context.set_screen(ScreenBuilder::new("probe").top_bar("Probe").text("Probe").build());
    }
    fn on_action(&mut self, _context: &mut Context, _action: ActionId) {}
}

pub fn run_baseline() {
    let _ = kobo_sdk::run("probe", Baseline);
}

pub fn input() -> Option<Vec<u8>> {
    let path = std::env::args().nth(1)?;
    std::fs::read(path).ok()
}
