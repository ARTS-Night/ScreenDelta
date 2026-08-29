use crate::{
    CaptureConfig, CaptureError, CaptureSource, CaptureStats, CaptureUpdate, CpuFrame, DeltaFrame,
    DeltaRegion, Frame, MonitorId, MonitorInfo, PixelFormat, Region,
};
use std::{
    mem::zeroed,
    slice,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HMODULE, RECT},
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
                CreateDXGIFactory1, DXGI_ERROR_MORE_DATA, DXGI_ERROR_WAIT_TIMEOUT,
                DXGI_OUTDUPL_MOVE_RECT, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
                IDXGIOutputDuplication,
            },
        },
    },
    core::Interface,
};

pub(crate) struct Session {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging: ID3D11Texture2D,
    region_staging: Vec<RegionStaging>,
    next_region_staging: usize,
    region: Region,
    output_region: Region,
    started: Instant,
    emitted_initial: bool,
    next_index: u64,
    last_pointer_update_time: i64,
    stats: CaptureStats,
}

struct RegionStaging {
    width: u32,
    height: u32,
    texture: ID3D11Texture2D,
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
            device,
            context,
            duplication,
            staging: staging.ok_or_else(|| CaptureError::new("D3D11 staging texture missing"))?,
            region_staging: Vec::new(),
            next_region_staging: 0,
            region: t.region,
            output_region: t.output_region,
            started: Instant::now(),
            emitted_initial: false,
            next_index: 0,
            last_pointer_update_time: 0,
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
        self.stats.poll_attempts += 1;
        let acquire_started = Instant::now();
        let mut info = unsafe { zeroed() };
        let mut resource = None;
        let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe {
            self.duplication
                .AcquireNextFrame(milliseconds, &mut info, &mut resource)
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                self.stats.unchanged_polls += 1;
                self.stats.acquire_wait += acquire_started.elapsed();
                return Ok(None);
            }
            Err(error) => return Err(win_error(error)),
        }
        self.stats.os_frames_acquired += 1;
        self.stats.acquire_wait += acquire_started.elapsed();
        let dirty = match self.dirty_regions() {
            Ok(dirty) => dirty,
            Err(error) => {
                unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
                return Err(error);
            }
        };
        if !dirty.is_empty()
            && !dirty
                .iter()
                .any(|damage| damage.intersection(self.region).is_some())
        {
            self.stats.region_skipped_updates += 1;
            unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
            return Ok(None);
        }
        let readback_started = Instant::now();
        let result = (|| {
            let texture: ID3D11Texture2D = resource
                .ok_or_else(|| CaptureError::new("DXGI returned no frame"))?
                .cast()
                .map_err(win_error)?;
            let b = D3D11_BOX {
                left: (self.region.x - self.output_region.x) as u32,
                top: (self.region.y - self.output_region.y) as u32,
                front: 0,
                right: (self.region.x - self.output_region.x) as u32 + self.region.size.width,
                bottom: (self.region.y - self.output_region.y) as u32 + self.region.size.height,
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
            self.stats.full_updates += 1;
            self.stats.full_payload_bytes += data.len() as u64;
            if !self.emitted_initial {
                self.stats.full_initial_updates += 1;
            }
            self.stats.readback += readback_started.elapsed();
            let frame = Frame {
                timestamp: self.started.elapsed(),
                index: self.next_index,
                region: self.region,
                cpu: CpuFrame {
                    width: self.region.size.width,
                    height: self.region.size.height,
                    stride: row,
                    format: PixelFormat::Bgra8,
                    data,
                },
            };
            self.next_index += 1;
            self.emitted_initial = true;
            Ok(frame)
        })();
        unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
        result.map(Some)
    }

    pub(crate) fn try_next_update(
        &mut self,
        timeout: Duration,
    ) -> Result<CaptureUpdate, CaptureError> {
        self.stats.poll_attempts += 1;
        let acquire_started = Instant::now();
        let mut info = unsafe { zeroed() };
        let mut resource = None;
        let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe {
            self.duplication
                .AcquireNextFrame(milliseconds, &mut info, &mut resource)
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                self.stats.unchanged_polls += 1;
                self.stats.unchanged_updates += 1;
                self.stats.acquire_wait += acquire_started.elapsed();
                return Ok(CaptureUpdate::Unchanged {
                    timestamp: self.started.elapsed(),
                    index: self.next_index,
                });
            }
            Err(error) => return Err(win_error(error)),
        }
        self.stats.os_frames_acquired += 1;
        if info.LastMouseUpdateTime != 0
            && info.LastMouseUpdateTime != self.last_pointer_update_time
        {
            self.last_pointer_update_time = info.LastMouseUpdateTime;
            self.stats.pointer_updates += 1;
            if info.PointerPosition.Visible.as_bool() {
                self.stats.separate_pointer_updates += 1;
            }
            if info.PointerShapeBufferSize != 0 {
                self.stats.pointer_shape_updates += 1;
            }
        }
        self.stats.acquire_wait += acquire_started.elapsed();
        let moves = match self.move_rects() {
            Ok(moves) => moves,
            Err(error) => {
                unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
                return Err(error);
            }
        };
        self.stats.move_rects_observed += moves.len() as u64;
        let dirty = match self.dirty_regions() {
            Ok(dirty) => dirty,
            Err(error) => {
                unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
                return Err(error);
            }
        };
        let relevant: Vec<_> = dirty
            .iter()
            .filter_map(|damage| damage.intersection(self.region))
            .collect();
        if !dirty.is_empty() && relevant.is_empty() {
            self.stats.region_skipped_updates += 1;
            self.stats.unchanged_updates += 1;
            unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
            return Ok(CaptureUpdate::Unchanged {
                timestamp: self.started.elapsed(),
                index: self.next_index,
            });
        }
        let result = (|| {
            let texture: ID3D11Texture2D = resource
                .ok_or_else(|| CaptureError::new("DXGI returned no frame"))?
                .cast()
                .map_err(win_error)?;
            let canvas_pixels =
                u64::from(self.region.size.width) * u64::from(self.region.size.height);
            let dirty_pixels: u64 = relevant
                .iter()
                .map(|region| u64::from(region.size.width) * u64::from(region.size.height))
                .sum();
            let full_reason = if !self.emitted_initial {
                Some(FullReason::Initial)
            } else if relevant.is_empty() {
                // Empty dirty metadata may accompany DXGI move metadata, which
                // is not represented as a public update yet.
                Some(FullReason::EmptyDamage)
            } else if relevant.len() > 32 {
                Some(FullReason::FragmentedDamage)
            } else if dirty_pixels.saturating_mul(2) >= canvas_pixels {
                Some(FullReason::LargeDamage)
            } else {
                None
            };
            let use_delta = full_reason.is_none();
            if use_delta {
                let readback_started = Instant::now();
                let mut regions = Vec::with_capacity(relevant.len());
                for global in relevant {
                    let pixels = self.readback_region(&texture, global)?;
                    regions.push(DeltaRegion {
                        region: Region::new(
                            global.x - self.region.x,
                            global.y - self.region.y,
                            global.size.width,
                            global.size.height,
                        )
                        .expect("intersection cannot have negative local coordinates"),
                        pixels,
                    });
                }
                self.stats.frames_captured += 1;
                self.stats.delta_updates += 1;
                self.stats.delta_regions += regions.len() as u64;
                self.stats.delta_payload_bytes += regions
                    .iter()
                    .map(|region| region.pixels.data.len() as u64)
                    .sum::<u64>();
                self.stats.readback += readback_started.elapsed();
                let update = CaptureUpdate::Delta(DeltaFrame {
                    timestamp: self.started.elapsed(),
                    index: self.next_index,
                    canvas: self.region.size,
                    regions,
                });
                self.next_index += 1;
                Ok(update)
            } else {
                let readback_started = Instant::now();
                let frame = self.readback_full(&texture)?;
                self.stats.frames_captured += 1;
                self.stats.full_updates += 1;
                self.stats.full_payload_bytes += frame.data.len() as u64;
                match full_reason.expect("Full fallback needs a reason") {
                    FullReason::Initial => self.stats.full_initial_updates += 1,
                    FullReason::EmptyDamage => self.stats.full_empty_damage_updates += 1,
                    FullReason::LargeDamage => self.stats.full_large_damage_updates += 1,
                    FullReason::FragmentedDamage => self.stats.full_fragmented_damage_updates += 1,
                }
                self.stats.readback += readback_started.elapsed();
                self.emitted_initial = true;
                let update = CaptureUpdate::Full(Frame {
                    timestamp: self.started.elapsed(),
                    index: self.next_index,
                    region: self.region,
                    cpu: frame,
                });
                self.next_index += 1;
                Ok(update)
            }
        })();
        unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
        result
    }
    pub(crate) fn stats(&self) -> CaptureStats {
        self.stats
    }

    fn readback_full(&self, texture: &ID3D11Texture2D) -> Result<CpuFrame, CaptureError> {
        self.readback_into(texture, self.region, &self.staging)
    }

    fn readback_region(
        &mut self,
        texture: &ID3D11Texture2D,
        region: Region,
    ) -> Result<CpuFrame, CaptureError> {
        let mut staging_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { self.staging.GetDesc(&mut staging_desc) };
        let format = staging_desc.Format;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: region.size.width,
            Height: region.size.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let staging = self.staging_for(region.size.width, region.size.height, &desc)?;
        self.readback_into(texture, region, &staging)
    }

    fn staging_for(
        &mut self,
        width: u32,
        height: u32,
        desc: &D3D11_TEXTURE2D_DESC,
    ) -> Result<ID3D11Texture2D, CaptureError> {
        if let Some(staging) = self
            .region_staging
            .iter()
            .find(|staging| staging.width == width && staging.height == height)
        {
            return Ok(staging.texture.clone());
        }
        let mut texture = None;
        unsafe { self.device.CreateTexture2D(desc, None, Some(&mut texture)) }
            .map_err(win_error)?;
        let texture =
            texture.ok_or_else(|| CaptureError::new("D3D11 delta staging texture missing"))?;
        self.stats.delta_staging_allocations += 1;
        let entry = RegionStaging {
            width,
            height,
            texture: texture.clone(),
        };
        const REGION_STAGING_CACHE_CAPACITY: usize = 8;
        if self.region_staging.len() < REGION_STAGING_CACHE_CAPACITY {
            self.region_staging.push(entry);
        } else {
            let slot = self.next_region_staging % REGION_STAGING_CACHE_CAPACITY;
            self.region_staging[slot] = entry;
            self.next_region_staging = self.next_region_staging.wrapping_add(1);
        }
        Ok(texture)
    }

    fn readback_into(
        &self,
        texture: &ID3D11Texture2D,
        region: Region,
        staging: &ID3D11Texture2D,
    ) -> Result<CpuFrame, CaptureError> {
        let x = (region.x - self.output_region.x) as u32;
        let y = (region.y - self.output_region.y) as u32;
        let b = D3D11_BOX {
            left: x,
            top: y,
            front: 0,
            right: x + region.size.width,
            bottom: y + region.size.height,
            back: 1,
        };
        unsafe {
            self.context
                .CopySubresourceRegion(staging, 0, 0, 0, 0, texture, 0, Some(&b))
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(win_error)?;
        let row = region.size.width as usize * 4;
        let mut data = vec![0; row * region.size.height as usize];
        for y in 0..region.size.height as usize {
            let src = unsafe {
                slice::from_raw_parts(
                    (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                    row,
                )
            };
            data[y * row..(y + 1) * row].copy_from_slice(src);
        }
        unsafe { self.context.Unmap(staging, 0) };
        Ok(CpuFrame {
            width: region.size.width,
            height: region.size.height,
            stride: row,
            format: PixelFormat::Bgra8,
            data,
        })
    }

    fn dirty_regions(&self) -> Result<Vec<Region>, CaptureError> {
        let mut required = 0;
        let mut empty = RECT::default();
        match unsafe {
            self.duplication
                .GetFrameDirtyRects(0, &mut empty, &mut required)
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_MORE_DATA => {}
            Err(error) => return Err(win_error(error)),
        }
        if required == 0 {
            return Ok(Vec::new());
        }
        let count = required as usize / std::mem::size_of::<RECT>();
        let mut rects = vec![RECT::default(); count];
        unsafe {
            self.duplication
                .GetFrameDirtyRects(required, rects.as_mut_ptr(), &mut required)
        }
        .map_err(win_error)?;
        Ok(rects
            .into_iter()
            .filter_map(|rect| {
                Region::new(
                    self.output_region.x + rect.left,
                    self.output_region.y + rect.top,
                    (rect.right - rect.left) as u32,
                    (rect.bottom - rect.top) as u32,
                )
            })
            .collect())
    }

    fn move_rects(&self) -> Result<Vec<DXGI_OUTDUPL_MOVE_RECT>, CaptureError> {
        let mut required = 0;
        let mut empty = DXGI_OUTDUPL_MOVE_RECT::default();
        match unsafe {
            self.duplication
                .GetFrameMoveRects(0, &mut empty, &mut required)
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_MORE_DATA => {}
            Err(error) => return Err(win_error(error)),
        }
        if required == 0 {
            return Ok(Vec::new());
        }
        let count = required as usize / std::mem::size_of::<DXGI_OUTDUPL_MOVE_RECT>();
        let mut rects = vec![DXGI_OUTDUPL_MOVE_RECT::default(); count];
        unsafe {
            self.duplication
                .GetFrameMoveRects(required, rects.as_mut_ptr(), &mut required)
        }
        .map_err(win_error)?;
        Ok(rects)
    }
}

#[derive(Clone, Copy)]
enum FullReason {
    Initial,
    EmptyDamage,
    LargeDamage,
    FragmentedDamage,
}

struct Target {
    adapter: IDXGIAdapter1,
    output: IDXGIOutput,
    output_region: Region,
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
                output_region: r,
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
