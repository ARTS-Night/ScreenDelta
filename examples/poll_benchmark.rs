use std::time::{Duration, Instant};

use screendelta::{CaptureConfig, CaptureSession, CaptureSource, FramePacer, Region, monitors};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let fps = args.get(1).map_or(Ok(15), |value| value.parse())?;
    let seconds = args.get(2).map_or(Ok(10), |value| value.parse())?;
    let monitor = monitors()?.remove(0);
    let source = match (args.get(3), args.get(4)) {
        (Some(width), Some(height)) => CaptureSource::Region(
            Region::new(0, 0, width.parse()?, height.parse()?).ok_or("Invalid region")?,
        ),
        _ => CaptureSource::Monitor(monitor.id),
    };
    let mut session = CaptureSession::start(CaptureConfig { source })?;
    let mut pacer = FramePacer::new(fps)?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        let _ = session.try_next_frame(Duration::ZERO)?;
        pacer.wait();
    }
    println!("fps={fps} seconds={seconds} stats={:?}", session.stats());
    Ok(())
}
