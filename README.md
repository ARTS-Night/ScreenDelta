# ScreenDelta

> Capture efficiently. Process only what changed.

ScreenDelta is a Rust 2024 library for Windows desktop capture. It keeps the
Windows/DXGI/D3D11 boundary private and exposes physical-pixel desktop regions,
monitor metadata, frames, CPU readback, and deadline-based frame pacing.

## Status

The current v0.1 foundation supports Windows DXGI Desktop Duplication for a
single monitor or a region completely contained in one monitor. Region cropping
happens on the GPU before the staging-texture CPU readback.

Multi-monitor composition, dirty/move metadata, cursor metadata, delta
detection, and non-Windows backends are intentionally not implemented yet.

## Example

```rust
use screendelta::{monitors, CaptureConfig, CaptureSession, CaptureSource};

let monitor = monitors()?.remove(0);
let mut capture = CaptureSession::start(CaptureConfig {
    source: CaptureSource::Monitor(monitor.id),
})?;
let pixels = capture.next_frame()?.readback()?;
println!("{} x {}", pixels.width, pixels.height);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Run `cargo run --example list_monitors` or `cargo run --example capture_region`
on a Windows desktop session to validate capture.

## Validation

`cargo fmt --check`, `cargo check`, and `cargo test` are expected to pass.
Desktop capture needs an interactive Windows session and is therefore an example
rather than a CI test.
