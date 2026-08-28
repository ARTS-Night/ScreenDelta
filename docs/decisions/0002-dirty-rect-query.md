# Dirty-rect query and region filtering

## Decision

After DXGI acquires an updated desktop frame, ScreenDelta reads dirty rectangles
before copying pixels. If every dirty rectangle is outside the requested
capture region, it releases the DXGI frame and returns no frame without a GPU
crop or CPU readback.

## Windows API detail

`GetFrameDirtyRects` needs a valid pointer even for the zero-byte size query on
this validation system, and a non-empty result reports `DXGI_ERROR_MORE_DATA`
with the required size. Both behaviours are handled explicitly before the
second call supplies the correctly sized buffer.

## Validation

On 2026-08-28, a 15 FPS, 5-second release benchmark completed after this
change: 76 polls, 14 readbacks, 62 DXGI timeouts, 18.0474 ms cumulative
readback. The earlier two implementations failed with `E_INVALIDARG` and then
`DXGI_ERROR_MORE_DATA`; those failures are retained here as API integration
evidence.
