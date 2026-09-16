//! Writes the tiny original study bundle for simulator routes and screenshots.
//!
//! The bundle is the same built-in sample the app offers on first use, built
//! with the same `encode` the host importer uses, so what the reader opens
//! here is what a staged import produces. Regenerate the committed fixture
//! with:
//!
//! ```sh
//! cargo run -p kobo-flashcards-format --example demo_bundle -- \
//!     scripts/fixtures/flashcards/collection.cobfc
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(output) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: demo_bundle OUTPUT.cobfc");
        return ExitCode::FAILURE;
    };
    if output.exists() {
        eprintln!("refusing to overwrite {}", output.display());
        return ExitCode::FAILURE;
    }
    match kobo_flashcards_format::sample_bundle() {
        Ok(bytes) => match std::fs::write(&output, bytes) {
            Ok(()) => {
                println!("wrote demo bundle to {}", output.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("could not write {}: {error}", output.display());
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("could not encode demo bundle: {error}");
            ExitCode::FAILURE
        }
    }
}
