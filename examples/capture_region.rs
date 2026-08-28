use screendelta::{CaptureConfig, CaptureSession, CaptureSource, monitors};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let monitor = monitors()?.remove(0);
    let mut capture = CaptureSession::start(CaptureConfig {
        source: CaptureSource::Monitor(monitor.id),
    })?;
    let frame = capture.next_frame()?.readback()?;
    println!(
        "captured {}x{} ({} bytes)",
        frame.width,
        frame.height,
        frame.data.len()
    );
    Ok(())
}
