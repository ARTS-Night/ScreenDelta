# Performance roadmap

This list is ordered by measured cost and implementation risk, rather than by
theoretical sophistication.

## 1. Dirty-rect intersection (next)

DXGI already reports non-overlapping regions changed since the previous frame.
When none intersect the requested capture region, ScreenDelta can return an
unchanged result without a GPU crop, staging copy, map, or CPU allocation.

**Why first:** this removes whole stages of work on the common idle or
partially-changing desktop path. It is also native metadata, not a heuristic.

**Acceptance measure:** count avoided readbacks and compare CPU bytes copied
with the same scripted desktop activity.

## 2. Bounded encoder queue (before longer recordings)

Move GIF work to one encoder worker and cap queued frames. On saturation, merge
duration into the latest frame or deliberately drop with timestamp accounting.

**Why second:** it bounds memory and keeps a future UI thread responsive. It is
not needed for the fixed three-second prototype, so adding it now would add
threading complexity without a user benefit.

## 3. Reusable GPU surfaces / asynchronous readback

Use a small staging-texture ring to allow a GPU copy to overlap a prior CPU
map. Measure first: `CopySubresourceRegion` is asynchronous, so a single
immediate map can create a pipeline stall.

**Why deferred:** it adds resource lifetime complexity and only helps if timing
shows readback is the bottleneck.

## Rejected for now

- CPU hashing / tile comparison: duplicates work that DXGI dirty rectangles
  already provide.
- GPU delta shaders: useful only after dirty-rect measurements demonstrate a
  bottleneck.
- A new GIF encoder dependency: does not remove capture/readback cost.

## Sources

- Microsoft documents that Desktop Duplication provides non-overlapping dirty
  regions through `GetFrameDirtyRects`.
- `CopySubresourceRegion` is a GPU copy and may be queued asynchronously; a
  staging resource exists specifically for GPU/CPU transfer.

See the linked Microsoft documentation in the repository README history or:
https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api
