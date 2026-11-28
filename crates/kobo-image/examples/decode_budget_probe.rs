//! Time public image decode entry points on a caller-supplied local file.
use std::{env, fs, time::Instant};
fn main() {
    let bytes = fs::read(env::args().nth(1).expect("local image path")).expect("read image");
    for colour in [false, true] {
        let started = Instant::now();
        let result = if colour { kobo_image::decode_colour(&bytes) } else { kobo_image::decode(&bytes) };
        println!("colour={colour} elapsed_us={} result={:?}", started.elapsed().as_micros(), result.as_ref().map(|p| (p.width(), p.height())));
    }
}
