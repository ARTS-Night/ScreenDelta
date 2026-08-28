use crate::{
    CaptureConfig, CaptureError, CaptureSource, CaptureStats, CpuFrame, Frame, MonitorId,
    MonitorInfo, PixelFormat, Region,
};
use std::{
    mem::zeroed,
    slice,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dxgi::Common::DXGI_SAMPLE_DESC,
            Dxgi::{
                CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, IDXGIAdapter1, IDXGIFactory1,
                IDXGIOutput, IDXGIOutput1, IDXGIOutputDuplication,
            },
        },
    },
    core::Interface,
};

pub(crate) struct Session {
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging: ID3D11Texture2D,
    region: Region,
    started: Instant,
    stats: CaptureStats,
}
impl Session {
    pub(crate) fn start(config: CaptureConfig) -> Result<Self, CaptureError> {
        let t = find_output(config.source)?;
        let (device, context) = device(&t.adapter)?;
        let output: IDXGIOutput1 = t.output.cast().map_err(win_error)?;
        let duplication = unsafe { output.DuplicateOutput(&device) }.map_err(win_error)?;
        let d = unsafe { duplication.GetDesc() };
        let desc = D3D11_TEXTURE2D_DESC {
            Width: t.region.size.width,
            Height: t.region.size.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: d.ModeDesc.Format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(win_error)?;
        Ok(Self {
            context,
            duplication,
            staging: staging.ok_or_else(|| CaptureError::new("D3D11 staging texture missing"))?,
            region: t.region,
            started: Instant::now(),
            stats: CaptureStats::default(),
        })
    }
    pub(crate) fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        loop {
            if let Some(frame) = self.try_next_frame(Duration::from_secs(1))? {
                return Ok(frame);
            }
        }
    }
    pub(crate) fn try_next_frame(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Frame>, CaptureError> {
        let mut info = unsafe { zeroed() };
        let mut resource = None;
        let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe {
            self.duplication
                .AcquireNextFrame(milliseconds, &mut info, &mut resource)
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(error) => return Err(win_error(error)),
        }
        let result = (|| {
            let texture: ID3D11Texture2D = resource
                .ok_or_else(|| CaptureError::new("DXGI returned no frame"))?
                .cast()
                .map_err(win_error)?;
            let b = D3D11_BOX {
                left: self.region.x as u32,
                top: self.region.y as u32,
                front: 0,
                right: self.region.x as u32 + self.region.size.width,
                bottom: self.region.y as u32 + self.region.size.height,
                back: 1,
            };
            unsafe {
                self.context
                    .CopySubresourceRegion(&self.staging, 0, 0, 0, 0, &texture, 0, Some(&b))
            };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe {
                self.context
                    .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            }
            .map_err(win_error)?;
            let row = self.region.size.width as usize * 4;
            let mut data = vec![0; row * self.region.size.height as usize];
            for y in 0..self.region.size.height as usize {
                let src = unsafe {
                    slice::from_raw_parts(
                        (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                        row,
                    )
                };
                data[y * row..(y + 1) * row].copy_from_slice(src);
            }
            unsafe { self.context.Unmap(&self.staging, 0) };
            self.stats.frames_captured += 1;
            Ok(Frame {
                timestamp: self.started.elapsed(),
                index: self.stats.frames_captured - 1,
                region: self.region,
                cpu: CpuFrame {
                    width: self.region.size.width,
                    height: self.region.size.height,
                    stride: row,
                    format: PixelFormat::Bgra8,
                    data,
                },
            })
        })();
        unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
        result.map(Some)
    }
    pub(crate) fn stats(&self) -> CaptureStats {
        self.stats
    }
}
struct Target {
    adapter: IDXGIAdapter1,
    output: IDXGIOutput,
    region: Region,
}
fn find_output(source: CaptureSource) -> Result<Target, CaptureError> {
    let f = factory()?;
    for a in 0.. {
        let Ok(adapter) = (unsafe { f.EnumAdapters1(a) }) else {
            break;
        };
        for o in 0.. {
            let Ok(output) = (unsafe { adapter.EnumOutputs(o) }) else {
                break;
            };
            let d = unsafe { output.GetDesc() }.map_err(win_error)?;
            let r = Region::new(
                d.DesktopCoordinates.left,
                d.DesktopCoordinates.top,
                (d.DesktopCoordinates.right - d.DesktopCoordinates.left) as u32,
                (d.DesktopCoordinates.bottom - d.DesktopCoordinates.top) as u32,
            )
            .unwrap();
            let region = match &source {
                CaptureSource::Monitor(id) if id.as_str() == format!("dxgi:{a}:{o}") => r,
                CaptureSource::Region(x) if x.intersection(r) == Some(*x) => *x,
                _ => continue,
            };
            return Ok(Target {
                adapter,
                output,
                region,
            });
        }
    }
    Err(CaptureError::new(
        "Capture source must fit one active monitor",
    ))
}
pub(crate) fn monitors() -> Result<Vec<MonitorInfo>, CaptureError> {
    let f = factory()?;
    let mut v = Vec::new();
    for a in 0.. {
        let Ok(adapter) = (unsafe { f.EnumAdapters1(a) }) else {
            break;
        };
        for o in 0.. {
            let Ok(output) = (unsafe { adapter.EnumOutputs(o) }) else {
                break;
            };
            let d = unsafe { output.GetDesc() }.map_err(win_error)?;
            let q = d.DesktopCoordinates;
            let region = Region::new(
                q.left,
                q.top,
                (q.right - q.left) as u32,
                (q.bottom - q.top) as u32,
            )
            .unwrap();
            let n = d.DeviceName.iter().position(|&x| x == 0).unwrap_or(32);
            v.push(MonitorInfo {
                id: MonitorId(format!("dxgi:{a}:{o}")),
                name: String::from_utf16_lossy(&d.DeviceName[..n]),
                primary: q.left == 0 && q.top == 0,
                region,
            })
        }
    }
    if v.is_empty() {
        Err(CaptureError::new("No active DXGI outputs found"))
    } else {
        Ok(v)
    }
}
fn factory() -> Result<IDXGIFactory1, CaptureError> {
    unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.map_err(win_error)
}
fn device(a: &IDXGIAdapter1) -> Result<(ID3D11Device, ID3D11DeviceContext), CaptureError> {
    let (mut d, mut c) = (None, None);
    unsafe {
        D3D11CreateDevice(
            a,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d),
            None,
            Some(&mut c),
        )
    }
    .map_err(win_error)?;
    Ok((
        d.ok_or_else(|| CaptureError::new("D3D11 device missing"))?,
        c.ok_or_else(|| CaptureError::new("D3D11 context missing"))?,
    ))
}
fn win_error(e: windows::core::Error) -> CaptureError {
    CaptureError::new(e.to_string())
}
