use std::time::{Duration, Instant};

use screendelta::{CaptureConfig, CaptureSession, CaptureSource, FramePacer, monitors};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fps = std::env::args()
        .nth(1)
        .map_or(Ok(15), |value| value.parse())?;
    let seconds = std::env::args()
        .nth(2)
        .map_or(Ok(10), |value| value.parse())?;
    let monitor = monitors()?.remove(0);
    let mut session = CaptureSession::start(CaptureConfig {
        source: CaptureSource::Monitor(monitor.id),
    })?;
    let mut pacer = FramePacer::new(fps)?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        let _ = session.try_next_frame(Duration::ZERO)?;
        pacer.wait();
    }
    println!("fps={fps} seconds={seconds} stats={:?}", session.stats());
    Ok(())
}
