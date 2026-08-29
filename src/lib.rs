//! Reusable desktop capture primitives.
//!
//! This initial release supports Windows only. Its public API deliberately
//! exposes no Windows or Direct3D types.

mod geometry;
mod pacing;

#[cfg(target_os = "windows")]
mod windows;

pub use geometry::{Region, Size};
pub use pacing::FramePacer;

use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MonitorId(String);

impl MonitorId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub name: String,
    pub region: Region,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Bgra8,
}

#[derive(Clone, Debug)]
pub struct CpuFrame {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Frame {
    timestamp: Duration,
    index: u64,
    region: Region,
    cpu: CpuFrame,
}

#[derive(Clone, Debug)]
pub struct DeltaRegion {
    pub region: Region,
    pub pixels: CpuFrame,
}

#[derive(Clone, Debug)]
pub struct DeltaFrame {
    pub timestamp: Duration,
    pub index: u64,
    /// Size of the capture canvas that this update modifies.
    pub canvas: Size,
    /// Regions use capture-local coordinates, so `(0, 0)` is the top-left of
    /// the configured capture source even when the monitor has a negative
    /// virtual-desktop position.
    pub regions: Vec<DeltaRegion>,
}

#[derive(Clone, Debug)]
pub enum CaptureUpdate {
    Full(Frame),
    Delta(DeltaFrame),
    Unchanged { timestamp: Duration, index: u64 },
}

impl Frame {
    pub fn timestamp(&self) -> Duration {
        self.timestamp
    }
    pub fn index(&self) -> u64 {
        self.index
    }
    pub fn region(&self) -> Region {
        self.region
    }
    pub fn readback(&self) -> Result<CpuFrame, CaptureError> {
        Ok(self.cpu.clone())
    }

    /// Consumes the frame and transfers its CPU buffer without another copy.
    pub fn into_readback(self) -> Result<CpuFrame, CaptureError> {
        Ok(self.cpu)
    }
}

#[derive(Clone, Debug)]
pub enum CaptureSource {
    Monitor(MonitorId),
    Region(Region),
}

/// Whether desktop-duplication pointer shapes are composited into delivered
/// CPU pixels. This is independent of any application-specific cursor UI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorCapture {
    #[default]
    Exclude,
    Include,
    /// Best-effort rendering with a matching Windows standard cursor shape.
    System,
}

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub source: CaptureSource,
    pub cursor: CursorCapture,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureStats {
    /// Frames acquired from DXGI before ScreenDelta's filtering.
    pub os_frames_acquired: u64,
    /// Desktop frames DXGI had already coalesced before acquisition.
    /// This is distinct from ScreenDelta's intentional unchanged/delta filtering.
    pub os_frames_coalesced: u64,
    pub frames_captured: u64,
    pub full_updates: u64,
    pub full_initial_updates: u64,
    pub full_empty_damage_updates: u64,
    pub full_large_damage_updates: u64,
    pub full_fragmented_damage_updates: u64,
    pub delta_updates: u64,
    pub delta_regions: u64,
    /// D3D staging textures allocated for variable-size Delta readbacks.
    /// A bounded cache keeps this low without retaining arbitrary sizes.
    pub delta_staging_allocations: u64,
    pub move_rects_observed: u64,
    /// Candidate regions produced from DXGI move metadata (both the exposed
    /// source and updated destination areas).
    pub move_damage_regions: u64,
    pub pointer_updates: u64,
    pub separate_pointer_updates: u64,
    pub pointer_shape_updates: u64,
    pub cursor_damage_regions: u64,
    pub cursor_composited_updates: u64,
    /// DXGI acquisitions that changed only pointer metadata. The desktop
    /// texture has no matching pixel damage, so they are not full read back.
    pub pointer_only_updates: u64,
    pub full_payload_bytes: u64,
    pub delta_payload_bytes: u64,
    pub unchanged_updates: u64,
    pub poll_attempts: u64,
    pub unchanged_polls: u64,
    pub region_skipped_updates: u64,
    pub acquire_wait: Duration,
    pub readback: Duration,
}

#[derive(Debug)]
pub struct CaptureError(String);

impl CaptureError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CaptureError {}

pub struct CaptureSession {
    #[cfg(target_os = "windows")]
    inner: windows::Session,
}

impl CaptureSession {
    pub fn start(config: CaptureConfig) -> Result<Self, CaptureError> {
        #[cfg(target_os = "windows")]
        return windows::Session::start(config).map(|inner| Self { inner });
        #[cfg(not(target_os = "windows"))]
        {
            let _ = config;
            Err(CaptureError::new("ScreenDelta v0.1 supports Windows only"))
        }
    }

    pub fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        #[cfg(target_os = "windows")]
        return self.inner.next_frame();
        #[cfg(not(target_os = "windows"))]
        Err(CaptureError::new("ScreenDelta v0.1 supports Windows only"))
    }

    /// Returns `Ok(None)` when the desktop did not change before `timeout`.
    pub fn try_next_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CaptureError> {
        #[cfg(target_os = "windows")]
        return self.inner.try_next_frame(timeout);
        #[cfg(not(target_os = "windows"))]
        {
            let _ = timeout;
            Err(CaptureError::new("ScreenDelta v0.1 supports Windows only"))
        }
    }

    /// Returns the most compact update that represents the newest desktop
    /// state. The first delivered update is always [`CaptureUpdate::Full`].
    /// Subsequent small DXGI dirty regions are returned in capture-local
    /// coordinates; a large or uncertain change falls back to `Full`.
    pub fn try_next_update(&mut self, timeout: Duration) -> Result<CaptureUpdate, CaptureError> {
        #[cfg(target_os = "windows")]
        return self.inner.try_next_update(timeout);
        #[cfg(not(target_os = "windows"))]
        {
            let _ = timeout;
            Err(CaptureError::new("ScreenDelta v0.1 supports Windows only"))
        }
    }

    pub fn stats(&self) -> CaptureStats {
        #[cfg(target_os = "windows")]
        return self.inner.stats();
        #[cfg(not(target_os = "windows"))]
        CaptureStats::default()
    }
}

pub fn monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
    #[cfg(target_os = "windows")]
    return windows::monitors();
    #[cfg(not(target_os = "windows"))]
    Err(CaptureError::new("ScreenDelta v0.1 supports Windows only"))
}
