# Data path audit — 2026-08-28

## Observed path

`AcquireNextFrame` returns a DXGI desktop texture. ScreenDelta queries dirty
rectangles, skips an update outside the requested region, otherwise performs:

```text
DXGI texture -> CopySubresourceRegion -> reusable staging texture -> Map
-> newly allocated Vec<u8> -> Frame -> consumer-owned CpuFrame
```

The crop is one GPU-to-GPU copy. Mapping copies `width * height * 4` bytes from
the staging mapping into a newly allocated CPU vector. `into_readback` then
moves that vector to the caller; it does not clone it.

## Allocation / copy audit

| Operation | Current cadence | Why | Avoidable now? |
| --- | --- | --- | --- |
| staging texture | session start | CPU-readable D3D11 resource | reused |
| dirty rect Vec | received DXGI update | DXGI metadata query | yes, reuse later if measured |
| CPU pixel Vec | delivered frame | safe owned CPU payload | only by a lazy/GPU transport |
| mapped-memory to Vec copy | delivered frame | mapping lifetime ends before Frame escapes | not safely in current API |

No process boundary exists: QuickGIFlick links ScreenDelta by Rust path
dependency in the same process. IPC/shared memory is therefore rejected before
measurement.

## Candidate transports

1. Full CPU frame (current): simple, safe fallback; expensive for motion.
2. Delta CPU region: dirty-region readback; expected to reduce small-motion
   bandwidth. Needs correctness experiment before adoption.
3. GPU handle/lazy readback: potentially lowest transfer cost, but adds D3D11
   lifetime complexity. Deferred until Plan 2 is measured.
