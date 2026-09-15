//! The launchable payload for the signed Store fixture.
//!
//! The original fixture payload was inert bytes: fine for package
//! transaction checks, silent about whether the package could ever run. The
//! launch canary needs a payload that actually speaks the protocol, so the
//! Store simulator journey packages this application and every install in
//! that journey proves handshake and first screen against the live
//! simulator, exactly as the device runtime requires on hardware.

use kobo_sdk::prelude::*;
use std::process::ExitCode;

struct QualityFixture;

impl KoboApp for QualityFixture {
    fn on_action(&mut self, _context: &mut Context, _action: ActionId) {}

    fn on_start(&mut self, context: &mut Context) {
        context.set_screen(
            ScreenBuilder::new("quality-fixture")
                .heading("Quality fixture")
                .text("The signed Store fixture application.")
                .build(),
        );
    }
}

fn main() -> ExitCode {
    match kobo_sdk::run("quality-fixture", QualityFixture) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("quality-fixture: {error}");
            ExitCode::FAILURE
        }
    }
}
