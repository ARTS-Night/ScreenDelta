use crate::{
    CaptureConfig, CaptureError, CaptureSource, CaptureStats, CaptureUpdate, CpuFrame,
    CursorCapture, DeltaFrame, DeltaRegion, Frame, MonitorId, MonitorInfo, PixelFormat, Region,
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
                DXGI_OUTDUPL_MOVE_RECT, DXGI_OUTDUPL_POINTER_SHAPE_INFO,
                DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput,
                IDXGIOutput1, IDXGIOutputDuplication,
            },
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
                DIB_RGB_COLORS, DeleteDC, DeleteObject, HGDIOBJ, SelectObject,
            },
        },
        UI::WindowsAndMessaging::{
            CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo,
            GetSystemMetrics, HCURSOR, HICON, ICONINFO, IDC_APPSTARTING, IDC_ARROW, IDC_HAND,
            IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDC_WAIT,
            LoadCursorW, SM_CXCURSOR, SM_CYCURSOR,
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
    cursor: CursorState,
    capture_cursor: CursorCapture,
    previous: Option<CpuFrame>,
    stats: CaptureStats,
}

struct RegionStaging {
    width: u32,
    height: u32,
    texture: ID3D11Texture2D,
}

#[derive(Clone, Default)]
struct CursorState {
    position: Option<windows::Win32::Foundation::POINT>,
    shape: Option<CursorShape>,
}

#[derive(Clone)]
struct CursorShape {
    width: u32,
    height: u32,
    pitch: u32,
    hotspot: windows::Win32::Foundation::POINT,
    bgra: Vec<u8>,
}

impl CursorState {
    fn bounds(&self) -> Option<Region> {
        let position = self.position?;
        let shape = self.shape.as_ref()?;
        Region::new(
            position.x.saturating_sub(shape.hotspot.x),
            position.y.saturating_sub(shape.hotspot.y),
            shape.width,
            shape.height,
        )
    }
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
            cursor: CursorState::default(),
            capture_cursor: config.cursor,
            previous: None,
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
        self.stats.os_frames_coalesced += u64::from(info.AccumulatedFrames.saturating_sub(1));
        if let Err(error) = self.update_cursor(&info, !self.emitted_initial) {
            unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
            return Err(error);
        }
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
            let mut cpu = CpuFrame {
                width: self.region.size.width,
                height: self.region.size.height,
                stride: row,
                format: PixelFormat::Bgra8,
                data,
            };
            self.composite_cursor(&mut cpu, self.region);
            let frame = Frame {
                timestamp: self.started.elapsed(),
                index: self.next_index,
                region: self.region,
                cpu,
            };
            self.previous = Some(frame.cpu.clone());
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
        self.stats.os_frames_coalesced += u64::from(info.AccumulatedFrames.saturating_sub(1));
        let pointer_changed = info.LastMouseUpdateTime != 0
            && info.LastMouseUpdateTime != self.last_pointer_update_time;
        if pointer_changed {
            self.last_pointer_update_time = info.LastMouseUpdateTime;
            self.stats.pointer_updates += 1;
            if info.PointerPosition.Visible.as_bool() {
                self.stats.separate_pointer_updates += 1;
            }
            if info.PointerShapeBufferSize != 0 {
                self.stats.pointer_shape_updates += 1;
            }
        }
        let cursor_damage = match self.update_cursor(&info, pointer_changed) {
            Ok(damage) => damage,
            Err(error) => {
                unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
                return Err(error);
            }
        };
        self.stats.acquire_wait += acquire_started.elapsed();
        let moves = match self.move_rects() {
            Ok(moves) => moves,
            Err(error) => {
                unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
                return Err(error);
            }
        };
        self.stats.move_rects_observed += moves.len() as u64;
        let mut damage = match self.dirty_regions() {
            Ok(dirty) => dirty,
            Err(error) => {
                unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
                return Err(error);
            }
        };
        let move_damage = move_damage_regions(&moves, self.output_region);
        self.stats.move_damage_regions += move_damage.len() as u64;
        damage.extend(move_damage);
        self.stats.cursor_damage_regions += cursor_damage.len() as u64;
        damage.extend(cursor_damage);
        let relevant = merge_overlapping_regions(
            damage
                .iter()
                .filter_map(|damage| damage.intersection(self.region))
                .collect(),
        );
        if !damage.is_empty() && relevant.is_empty() {
            self.stats.region_skipped_updates += 1;
            self.stats.unchanged_updates += 1;
            unsafe { self.duplication.ReleaseFrame() }.map_err(win_error)?;
            return Ok(CaptureUpdate::Unchanged {
                timestamp: self.started.elapsed(),
                index: self.next_index,
            });
        }
        if damage.is_empty() {
            // Desktop Duplication reports pointer updates separately from the
            // texture. Until a caller asks for cursor composition, a
            // pointer-only acquisition has no desktop pixels to transport.
            if pointer_changed {
                self.stats.pointer_only_updates += 1;
            }
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
                    let mut pixels = self.readback_region(&texture, global)?;
                    self.composite_cursor(&mut pixels, global);
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
                let update = DeltaFrame {
                    timestamp: self.started.elapsed(),
                    index: self.next_index,
                    canvas: self.region.size,
                    regions,
                };
                self.apply_to_previous(&update.regions);
                self.next_index += 1;
                Ok(CaptureUpdate::Delta(update))
            } else {
                let readback_started = Instant::now();
                let mut frame = self.readback_full(&texture)?;
                self.composite_cursor(&mut frame, self.region);
                self.stats.frames_captured += 1;
                let reason = full_reason.expect("Full fallback needs a reason");
                if matches!(
                    reason,
                    FullReason::LargeDamage | FullReason::FragmentedDamage
                ) && self.previous.is_some()
                {
                    self.stats.verified_full_damage_updates += 1;
                    let verified = changed_tile_regions(
                        self.previous.as_ref().expect("previous canvas was checked"),
                        &frame,
                    );
                    if verified.is_empty() {
                        self.stats.verified_unchanged_updates += 1;
                        self.stats.unchanged_updates += 1;
                        self.stats.readback += readback_started.elapsed();
                        self.previous = Some(frame);
                        return Ok(CaptureUpdate::Unchanged {
                            timestamp: self.started.elapsed(),
                            index: self.next_index,
                        });
                    }
                    let verified_pixels: u64 = verified
                        .iter()
                        .map(|region| u64::from(region.size.width) * u64::from(region.size.height))
                        .sum();
                    if verified_pixels.saturating_mul(2) < canvas_pixels {
                        let regions = regions_from_full(&frame, &verified);
                        self.stats.delta_updates += 1;
                        self.stats.delta_regions += regions.len() as u64;
                        self.stats.delta_payload_bytes += regions
                            .iter()
                            .map(|region| region.pixels.data.len() as u64)
                            .sum::<u64>();
                        self.stats.readback += readback_started.elapsed();
                        self.previous = Some(frame);
                        let update = CaptureUpdate::Delta(DeltaFrame {
                            timestamp: self.started.elapsed(),
                            index: self.next_index,
                            canvas: self.region.size,
                            regions,
                        });
                        self.next_index += 1;
                        return Ok(update);
                    }
                }
                self.stats.full_updates += 1;
                self.stats.full_payload_bytes += frame.data.len() as u64;
                match reason {
                    FullReason::Initial => self.stats.full_initial_updates += 1,
                    FullReason::EmptyDamage => self.stats.full_empty_damage_updates += 1,
                    FullReason::LargeDamage => self.stats.full_large_damage_updates += 1,
                    FullReason::FragmentedDamage => self.stats.full_fragmented_damage_updates += 1,
                }
                self.stats.readback += readback_started.elapsed();
                self.emitted_initial = true;
                self.previous = Some(frame.clone());
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

    fn update_cursor(
        &mut self,
        info: &windows::Win32::Graphics::Dxgi::DXGI_OUTDUPL_FRAME_INFO,
        pointer_changed: bool,
    ) -> Result<Vec<Region>, CaptureError> {
        if self.capture_cursor == CursorCapture::Exclude {
            return Ok(Vec::new());
        }
        let before = self.cursor.bounds();
        let shape_changed = info.PointerShapeBufferSize != 0;
        match self.capture_cursor {
            CursorCapture::Include => {
                if shape_changed {
                    self.cursor.shape = self.read_cursor_shape(info.PointerShapeBufferSize)?;
                }
                if pointer_changed || shape_changed {
                    self.cursor.position = info
                        .PointerPosition
                        .Visible
                        .as_bool()
                        .then_some(info.PointerPosition.Position);
                }
            }
            CursorCapture::System if pointer_changed || shape_changed => {
                self.cursor = system_cursor()?;
            }
            CursorCapture::Exclude | CursorCapture::System => {}
        }
        let after = self.cursor.bounds();
        let mut damage = Vec::with_capacity(2);
        if before != after {
            damage.extend(before);
            damage.extend(after);
        } else if shape_changed {
            damage.extend(after);
        }
        Ok(damage)
    }

    fn read_cursor_shape(&self, size: u32) -> Result<Option<CursorShape>, CaptureError> {
        let mut data = vec![0; size as usize];
        let mut required = 0;
        let mut info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
        let read = unsafe {
            self.duplication.GetFramePointerShape(
                size,
                data.as_mut_ptr().cast(),
                &mut required,
                &mut info,
            )
        };
        if let Err(error) = read {
            if error.code() != DXGI_ERROR_MORE_DATA || required <= size {
                return Err(win_error(error));
            }
            data.resize(required as usize, 0);
            unsafe {
                self.duplication.GetFramePointerShape(
                    required,
                    data.as_mut_ptr().cast(),
                    &mut required,
                    &mut info,
                )
            }
            .map_err(win_error)?;
        }
        if info.Type != DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR.0 as u32
            || info.Pitch < info.Width.saturating_mul(4)
            || data.len() < info.Pitch as usize * info.Height as usize
        {
            return Ok(None);
        }
        data.truncate(info.Pitch as usize * info.Height as usize);
        Ok(Some(CursorShape {
            width: info.Width,
            height: info.Height,
            pitch: info.Pitch,
            hotspot: info.HotSpot,
            bgra: data,
        }))
    }

    fn composite_cursor(&mut self, frame: &mut CpuFrame, frame_region: Region) {
        let Some(bounds) = self.cursor.bounds() else {
            return;
        };
        if frame_region.intersection(bounds).is_none() {
            return;
        }
        let Some(shape) = self.cursor.shape.as_ref() else {
            return;
        };
        let mut composed = false;
        for y in 0..shape.height as i32 {
            let destination_y = bounds.y + y - frame_region.y;
            if !(0..frame.height as i32).contains(&destination_y) {
                continue;
            }
            for x in 0..shape.width as i32 {
                let destination_x = bounds.x + x - frame_region.x;
                if !(0..frame.width as i32).contains(&destination_x) {
                    continue;
                }
                let source = y as usize * shape.pitch as usize + x as usize * 4;
                let destination =
                    destination_y as usize * frame.stride + destination_x as usize * 4;
                composed |= blend_bgra(
                    &mut frame.data[destination..destination + 4],
                    &shape.bgra[source..source + 4],
                );
            }
        }
        if composed {
            self.stats.cursor_composited_updates += 1;
        }
    }

    fn readback_full(&self, texture: &ID3D11Texture2D) -> Result<CpuFrame, CaptureError> {
        self.readback_into(texture, self.region, &self.staging)
    }

    fn apply_to_previous(&mut self, regions: &[DeltaRegion]) {
        let Some(previous) = self.previous.as_mut() else {
            return;
        };
        for region in regions {
            let row = region.region.size.width as usize * 4;
            for y in 0..region.region.size.height as usize {
                let source = y * region.pixels.stride;
                let destination =
                    (region.region.y as usize + y) * previous.stride + region.region.x as usize * 4;
                previous.data[destination..destination + row]
                    .copy_from_slice(&region.pixels.data[source..source + row]);
            }
        }
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

fn blend_bgra(destination: &mut [u8], source: &[u8]) -> bool {
    let alpha = source[3] as u16;
    if alpha == 0 {
        return false;
    }
    for channel in 0..3 {
        let foreground = source[channel] as u16;
        let background = destination[channel] as u16;
        destination[channel] =
            ((foreground * alpha + background * (255 - alpha) + 127) / 255) as u8;
    }
    destination[3] = 255;
    true
}

fn system_cursor() -> Result<CursorState, CaptureError> {
    let mut info = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetCursorInfo(&mut info) }.map_err(win_error)?;
    if info.flags.0 & CURSOR_SHOWING.0 == 0 {
        return Ok(CursorState::default());
    }
    let cursor = matching_standard_cursor(info.hCursor)?;
    Ok(CursorState {
        position: Some(info.ptScreenPos),
        shape: Some(rasterize_cursor(cursor)?),
    })
}

fn matching_standard_cursor(current: HCURSOR) -> Result<HCURSOR, CaptureError> {
    for id in [
        IDC_ARROW,
        IDC_HAND,
        IDC_IBEAM,
        IDC_SIZEALL,
        IDC_SIZENESW,
        IDC_SIZENS,
        IDC_SIZENWSE,
        IDC_SIZEWE,
        IDC_WAIT,
        IDC_APPSTARTING,
    ] {
        let cursor = unsafe { LoadCursorW(None, id) }.map_err(win_error)?;
        if cursor.0 == current.0 {
            return Ok(cursor);
        }
    }
    unsafe { LoadCursorW(None, IDC_ARROW) }.map_err(win_error)
}

fn rasterize_cursor(cursor: HCURSOR) -> Result<CursorShape, CaptureError> {
    let width = unsafe { GetSystemMetrics(SM_CXCURSOR) }.max(1) as u32;
    let height = unsafe { GetSystemMetrics(SM_CYCURSOR) }.max(1) as u32;
    let mut icon = ICONINFO::default();
    unsafe { GetIconInfo(HICON(cursor.0), &mut icon) }.map_err(win_error)?;
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // A top-down DIB matches the CPU frame row order.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap =
        unsafe { CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) }
            .map_err(win_error)?;
    let dc = unsafe { CreateCompatibleDC(None) };
    let old = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
    let result = (|| {
        unsafe {
            DrawIconEx(
                dc,
                0,
                0,
                HICON(cursor.0),
                width as i32,
                height as i32,
                0,
                None,
                DI_NORMAL,
            )
        }
        .map_err(win_error)?;
        let bytes = width as usize * height as usize * 4;
        let bgra = unsafe { slice::from_raw_parts(bits.cast::<u8>(), bytes) }.to_vec();
        Ok(CursorShape {
            width,
            height,
            pitch: width * 4,
            hotspot: windows::Win32::Foundation::POINT {
                x: icon.xHotspot as i32,
                y: icon.yHotspot as i32,
            },
            bgra,
        })
    })();
    unsafe {
        let _ = SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        if !icon.hbmMask.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(icon.hbmMask.0));
        }
        if !icon.hbmColor.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(icon.hbmColor.0));
        }
    }
    result
}

const VERIFICATION_TILE: u32 = 64;

/// When a compositor reports the whole desktop as dirty, verify its pixels in
/// bounded tiles before retaining another full canvas. This keeps remote or
/// virtual desktops from turning a small visual change into full-frame storage.
fn changed_tile_regions(previous: &CpuFrame, current: &CpuFrame) -> Vec<Region> {
    if previous.width != current.width
        || previous.height != current.height
        || previous.format != current.format
    {
        return Region::new(0, 0, current.width, current.height)
            .into_iter()
            .collect();
    }
    let mut regions: Vec<Region> = Vec::new();
    for y in (0..current.height).step_by(VERIFICATION_TILE as usize) {
        let height = VERIFICATION_TILE.min(current.height - y);
        let mut x = 0;
        while x < current.width {
            let width = VERIFICATION_TILE.min(current.width - x);
            if !tile_changed(previous, current, x, y, width, height) {
                x += width;
                continue;
            }
            let start = x;
            x += width;
            while x < current.width {
                let next_width = VERIFICATION_TILE.min(current.width - x);
                if !tile_changed(previous, current, x, y, next_width, height) {
                    break;
                }
                x += next_width;
            }
            let width = x - start;
            if let Some(region) = regions.iter_mut().find(|region| {
                region.x == start as i32
                    && region.size.width == width
                    && region.y + region.size.height as i32 == y as i32
            }) {
                region.size.height += height;
            } else {
                regions.push(
                    Region::new(start as i32, y as i32, width, height)
                        .expect("non-empty changed tile run"),
                );
            }
        }
    }
    regions
}

fn tile_changed(
    previous: &CpuFrame,
    current: &CpuFrame,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> bool {
    let bytes = width as usize * 4;
    (0..height as usize).any(|row| {
        let offset = (y as usize + row) * current.stride + x as usize * 4;
        previous.data[offset..offset + bytes] != current.data[offset..offset + bytes]
    })
}

fn regions_from_full(frame: &CpuFrame, regions: &[Region]) -> Vec<DeltaRegion> {
    regions
        .iter()
        .map(|region| {
            let row = region.size.width as usize * 4;
            let mut data = vec![0; row * region.size.height as usize];
            for y in 0..region.size.height as usize {
                let source = (region.y as usize + y) * frame.stride + region.x as usize * 4;
                data[y * row..(y + 1) * row].copy_from_slice(&frame.data[source..source + row]);
            }
            DeltaRegion {
                region: *region,
                pixels: CpuFrame {
                    width: region.size.width,
                    height: region.size.height,
                    stride: row,
                    format: frame.format,
                    data,
                },
            }
        })
        .collect()
}

/// DXGI move metadata names the pixels copied to `DestinationRect`, but the
/// source rectangle is damaged too: it now contains the exposed background.
/// Both rectangles are relative to the output, so make them desktop regions
/// before intersecting the caller's capture area.
fn move_damage_regions(moves: &[DXGI_OUTDUPL_MOVE_RECT], output: Region) -> Vec<Region> {
    let mut damage = Vec::with_capacity(moves.len().saturating_mul(2));
    for moved in moves {
        let destination = moved.DestinationRect;
        let width = destination.right.saturating_sub(destination.left) as u32;
        let height = destination.bottom.saturating_sub(destination.top) as u32;
        let destination = Region::new(
            output.x.saturating_add(destination.left),
            output.y.saturating_add(destination.top),
            width,
            height,
        );
        let source = Region::new(
            output.x.saturating_add(moved.SourcePoint.x),
            output.y.saturating_add(moved.SourcePoint.y),
            width,
            height,
        );
        damage.extend(destination.into_iter().chain(source));
    }
    damage
}

fn merge_overlapping_regions(mut regions: Vec<Region>) -> Vec<Region> {
    let mut merged: Vec<Region> = Vec::with_capacity(regions.len());
    while let Some(mut region) = regions.pop() {
        let mut index = 0;
        while index < merged.len() {
            if merged[index].intersection(region).is_some() {
                region = region_union(merged.swap_remove(index), region);
                index = 0;
            } else {
                index += 1;
            }
        }
        merged.push(region);
    }
    merged
}

fn region_union(left: Region, right: Region) -> Region {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    let right_edge = (i64::from(left.x) + i64::from(left.size.width))
        .max(i64::from(right.x) + i64::from(right.size.width));
    let bottom_edge = (i64::from(left.y) + i64::from(left.size.height))
        .max(i64::from(right.y) + i64::from(right.size.height));
    Region::new(
        x,
        y,
        (right_edge - i64::from(x)) as u32,
        (bottom_edge - i64::from(y)) as u32,
    )
    .expect("union of non-empty regions is non-empty")
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::POINT;

    fn cpu_frame(width: u32, height: u32, value: u8) -> CpuFrame {
        CpuFrame {
            width,
            height,
            stride: width as usize * 4,
            format: PixelFormat::Bgra8,
            data: vec![value; width as usize * height as usize * 4],
        }
    }

    #[test]
    fn full_damage_verification_returns_only_changed_tiles() {
        let previous = cpu_frame(128, 128, 0);
        let mut current = previous.clone();
        current.data[(70 * current.stride) + 70 * 4] = 1;
        assert_eq!(
            changed_tile_regions(&previous, &current),
            vec![Region::new(64, 64, 64, 64).unwrap()]
        );
        assert!(changed_tile_regions(&previous, &previous).is_empty());
    }

    #[test]
    fn move_damage_covers_source_and_destination_then_merges_overlap() {
        let moved = DXGI_OUTDUPL_MOVE_RECT {
            SourcePoint: POINT { x: 10, y: 20 },
            DestinationRect: RECT {
                left: 30,
                top: 40,
                right: 50,
                bottom: 60,
            },
        };
        let output = Region::new(-100, 50, 1920, 1080).unwrap();
        assert_eq!(
            move_damage_regions(&[moved], output),
            vec![
                Region::new(-70, 90, 20, 20).unwrap(),
                Region::new(-90, 70, 20, 20).unwrap(),
            ]
        );
        assert_eq!(
            merge_overlapping_regions(vec![
                Region::new(0, 0, 10, 10).unwrap(),
                Region::new(5, 5, 10, 10).unwrap(),
                Region::new(10, 10, 10, 10).unwrap(),
            ]),
            vec![Region::new(0, 0, 20, 20).unwrap()]
        );
    }

    #[test]
    fn cursor_blending_preserves_background_for_transparent_pixels() {
        let mut destination = [10, 20, 30, 40];
        assert!(!blend_bgra(&mut destination, &[1, 2, 3, 0]));
        assert_eq!(destination, [10, 20, 30, 40]);
        assert!(blend_bgra(&mut destination, &[110, 120, 130, 128]));
        assert_eq!(destination, [60, 70, 80, 255]);
    }
}
