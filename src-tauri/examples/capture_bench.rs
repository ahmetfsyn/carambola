//! Temporary capture/encode benchmark used to measure the screenshot
//! activation pipeline on the current machine.
//!
//! Run with: cargo run --example capture_bench

use std::time::Instant;

fn main() {
    let runs = 10;
    let mut totals = Vec::new();

    // Warm up (first capture allocates the DIB buffers).
    let _ = carambola::capture::capture_screen();

    for i in 0..runs {
        let start = Instant::now();
        let frame = match carambola::capture::capture_screen() {
            Ok(frame) => frame,
            Err(err) => {
                eprintln!("capture run {} failed: {err}", i + 1);
                continue;
            }
        };
        let total_ms = start.elapsed().as_secs_f64() * 1000.0;
        totals.push(total_ms);
        println!(
            "run {:2}: total {:7.1} ms | capture {:6.1} ms | encode {:5.1} ms | png {:8} B | {}x{}",
            i + 1,
            total_ms,
            frame.capture_ms,
            frame.encode_ms,
            frame.png_bytes.len(),
            frame.width,
            frame.height
        );
    }

    let avg = totals.iter().sum::<f64>() / totals.len() as f64;
    let max = totals.iter().cloned().fold(0.0, f64::max);
    println!("capture+encode: average = {avg:.1} ms, max = {max:.1} ms");
}
