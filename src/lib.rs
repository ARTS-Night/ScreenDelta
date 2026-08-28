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
}

#[derive(Clone, Debug)]
pub enum CaptureSource {
    Monitor(MonitorId),
    Region(Region),
}

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub source: CaptureSource,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureStats {
    pub frames_captured: u64,
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
