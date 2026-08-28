# Data path audit — 2026-08-28

## Observed path

`AcquireNextFrame` returns a DXGI desktop texture. ScreenDelta queries dirty
rectangles. After an initial Full frame, small relevant damage is read back by
region; an update outside the requested region is Unchanged. Full remains the
fallback for initial, large, fragmented, and empty/uncertain DXGI damage.

```text
Full:
DXGI texture -> CopySubresourceRegion -> reusable staging texture -> Map
-> newly allocated Vec<u8> -> Frame -> consumer-owned CpuFrame

Small Delta:
DXGI texture -> CopySubresourceRegion (each dirty region) -> region staging
texture -> Map -> Vec<u8> only for that region -> DeltaFrame
```

The crop is one GPU-to-GPU copy. Mapping copies `width * height * 4` bytes from
the staging mapping into a newly allocated CPU vector. `into_readback` then
moves that vector to the caller; it does not clone it.

## Allocation / copy audit

| Operation | Current cadence | Why | Avoidable now? |
| --- | --- | --- | --- |
| staging texture | session start | CPU-readable D3D11 resource | reused |
| dirty rect Vec | received DXGI update | DXGI metadata query | yes, reuse later if measured |
| CPU pixel Vec | Full or changed region | safe owned CPU payload | only by a lazy/GPU transport |
| mapped-memory to Vec copy | delivered frame | mapping lifetime ends before Frame escapes | not safely in current API |

No process boundary exists: QuickGIFlick links ScreenDelta by Rust path
dependency in the same process. IPC/shared memory is therefore rejected before
measurement.

## Candidate transports

1. Full CPU frame: simple, safe fallback; expensive for motion.
2. Delta CPU region: implemented for at most 32 dirty regions totaling under
   half the canvas. The conservative threshold preserves a Full fallback while
   the hardware measurements are collected.
3. GPU handle/lazy readback: potentially lowest transfer cost, but adds D3D11
   lifetime complexity. Deferred until Plan 2 is measured.
