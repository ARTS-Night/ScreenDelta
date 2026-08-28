# Non-blocking frame polling

## Decision

`CaptureSession::try_next_frame(timeout)` returns `Ok(None)` when DXGI has no
desktop update within the supplied timeout. `next_frame()` remains a blocking
convenience method.

## Reason

DXGI Desktop Duplication signals only changed desktop frames. Treating its
normal wait timeout as a capture error made an idle desktop stop QuickGIFlick
recordings and made output FPS depend on desktop activity.

## Consequence

Consumers that own output timing can preserve elapsed time without inventing a
new capture frame. QuickGIFlick samples at its requested cadence and extends
the prior GIF frame duration when no desktop update arrives.
