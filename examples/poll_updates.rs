use std::time::{Duration, Instant};

use screendelta::{
    CaptureConfig, CaptureSession, CaptureSource, CaptureUpdate, CursorCapture, FramePacer,
    monitors,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fps = std::env::var("SCREENDELTA_FPS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(15);
    let seconds = std::env::var("SCREENDELTA_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5);
    let monitor = monitors()?
        .into_iter()
        .next()
        .ok_or("No monitor available")?;
    let mut capture = CaptureSession::start(CaptureConfig {
        source: CaptureSource::Monitor(monitor.id),
        cursor: CursorCapture::Exclude,
    })?;
    let mut full = 0;
    let mut delta = 0;
    let mut delta_bytes = 0usize;
    let mut unchanged = 0;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut pacer = FramePacer::new(fps)?;
    while Instant::now() < deadline {
        pacer.wait();
        match capture.try_next_update(Duration::ZERO)? {
            CaptureUpdate::Full(_) => full += 1,
            CaptureUpdate::Delta(update) => {
                delta += 1;
                delta_bytes += update
                    .regions
                    .iter()
                    .map(|region| region.pixels.data.len())
                    .sum::<usize>();
            }
            CaptureUpdate::Unchanged { .. } => unchanged += 1,
        }
    }
    println!(
        "full={full} delta={delta} delta_bytes={delta_bytes} unchanged={unchanged} stats={:?}",
        capture.stats()
    );
    Ok(())
}
