# ScreenDelta

> Capture efficiently. Process only what changed.

ScreenDelta is a Rust 2024 library for Windows desktop capture. It keeps the
Windows/DXGI/D3D11 boundary private and exposes physical-pixel desktop regions,
monitor metadata, Full/Delta/Unchanged updates, CPU readback, and deadline-based
frame pacing.

## Status

The current v0.1 foundation supports Windows DXGI Desktop Duplication for a
single monitor or a region completely contained in one monitor. Region cropping
happens on the GPU before the staging-texture CPU readback.

The first update is always `Full`. Later small DXGI dirty regions are copied and
read back independently as `Delta`; timeout and out-of-region damage become
`Unchanged`. Large, fragmented, empty, or otherwise uncertain damage falls back
to `Full`. Delta regions are relative to the selected capture canvas, not the
virtual-desktop origin.

Multi-monitor composition and non-Windows backends are intentionally not
implemented yet. DXGI move rectangles contribute source/destination Delta
candidates. `CursorCapture::Include` composites DXGI Color cursor shapes into
CPU Full/Delta pixels; unsupported Monochrome and Masked Color shapes remain
excluded rather than corrupting the canvas.
`CursorCapture::System` instead renders a matching Windows standard cursor
(Arrow, Hand, I-Beam, Resize, Busy, or Wait), with Arrow as a safe fallback.

## Example

```rust
use screendelta::{CursorCapture, monitors, CaptureConfig, CaptureSession, CaptureSource};

let monitor = monitors()?.remove(0);
let mut capture = CaptureSession::start(CaptureConfig {
    source: CaptureSource::Monitor(monitor.id),
    cursor: CursorCapture::Exclude,
})?;
let pixels = capture.next_frame()?.readback()?;
println!("{} x {}", pixels.width, pixels.height);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Run `cargo run --example list_monitors` or `cargo run --example capture_region`
on a Windows desktop session to validate capture.

## Delta consumer example

```rust
use std::time::Duration;
use screendelta::CaptureUpdate;

match capture.try_next_update(Duration::from_millis(16))? {
    CaptureUpdate::Full(frame) => {
        // Initialize the consumer canvas with frame.into_readback()?.
    }
    CaptureUpdate::Delta(delta) => {
        // Copy each delta.regions[n].pixels into its canvas-local region.
        println!("update {} has {} regions", delta.index, delta.regions.len());
    }
    CaptureUpdate::Unchanged { .. } => {
        // Extend presentation time; do not reprocess the same pixels.
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Run `cargo run --release --example poll_updates` to print the observed update
mix and capture timing on an interactive Windows desktop.

For debug diagnostics, use the non-optimized examples (the terminal remains
visible):

```powershell
cargo run --example list_monitors
cargo run --example poll_benchmark -- 15 10
cargo run --example controlled_stimulus -- small 3
```

`poll_benchmark` reports capture/readback timing and Full/Delta/Unchanged
counts. Keep the selected region on one monitor; DXGI capture requires an
interactive Windows session.

For repeatable Windows workload measurements, see
[`docs/controlled-benchmark.md`](docs/controlled-benchmark.md).

## API guide

`monitors()` returns the physical-pixel monitors available to the current
Windows session. Start a session with either a monitor ID or a region that is
fully contained by one monitor:

```rust
let monitor = screendelta::monitors()?.remove(0);
let mut session = screendelta::CaptureSession::start(
    screendelta::CaptureConfig {
        source: screendelta::CaptureSource::Monitor(monitor.id),
        cursor: screendelta::CursorCapture::Exclude,
    },
)?;
```

Use `next_frame()` when every acquired frame is needed. Use
`try_next_update(timeout)` for the delta pipeline. The first result is always
`CaptureUpdate::Full`; later results are `Delta` (changed, canvas-local
regions) or `Unchanged` (timestamp and index only). A `Frame` owns its CPU
buffer: `into_readback()` moves it, while `readback()` returns a compatibility
clone. `DeltaRegion::pixels` has the same `CpuFrame` BGRA8 format and stride
rules as a full frame.

`CaptureStats` is a cheap snapshot for diagnostics. It separates OS
acquisitions (including `os_frames_coalesced` before ScreenDelta sees them),
Full/Delta/Unchanged decisions, pointer-only updates, payload bytes, staging
allocations, acquire wait, and readback time. Call `stats()` after a run rather
than logging every frame.

When DXGI reports fragmented or near-full-screen damage after the initial
frame, ScreenDelta verifies 64×64 CPU tiles against its retained canvas. A
small verified change is delivered as `Delta`; an identical canvas is
`Unchanged`. `verified_full_damage_updates` and `verified_unchanged_updates`
show when this fallback was used.

`CursorCapture::Exclude` avoids cursor pixels, `Include` composites supported
DXGI Color shapes, and `System` draws a best-effort Windows standard cursor.
All public types are platform-neutral; Windows/DXGI handles never cross the
API boundary. `CaptureSession` and its frames are not `Send` promises and
should be owned by the capture worker that consumes them.

## Validation

`cargo fmt --check`, `cargo check`, and `cargo test` are expected to pass.
Desktop capture needs an interactive Windows session and is therefore an example
rather than a CI test.
