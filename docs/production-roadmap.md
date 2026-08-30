# Production roadmap

ScreenDelta remains a generic Windows capture and delta library. QuickGIFlick
is the consumer; GIF policy and UI behavior do not belong in this repository.

## Release candidate blockers (P0)

- Recover or return a clear error for DXGI Access Lost, display changes,
  sleep/resume, and monitor disconnect.
- Validate that a requested region is fully contained in one output. Negative
  desktop coordinates are supported; cross-output capture remains explicit.
- Preserve timestamp monotonicity and expose captured, unchanged, delta, and
  fallback statistics for diagnostics.
- Verify Original cursor handling for Color, Monochrome, and Masked Color
  pointer shapes, or provide an explicit safe fallback.
- Run Windows 10/11, multi-monitor, mixed-DPI, and negative-coordinate tests on
  real hardware before claiming those configurations are supported.

## Performance gates

- Dirty rectangles must avoid unnecessary crop, staging copy, and CPU readback
  when the requested region is untouched.
- Measure CPU readback, delta verification, allocation, and queue behavior at
  10/15/20/30 FPS for static, small-motion, scrolling, and full-motion scenes.
- Add asynchronous staging only if measurements show that synchronous mapping
  is the bottleneck.

## Deferred until measured

- GPU comparison shaders and unsafe SIMD.
- Cross-output capture implementation.
- New dependencies for pooling or scheduling.

## Release gates

The public API must represent Full, Delta, and Unchanged updates without
Windows-only GIF concepts, with clear timestamp and ownership semantics. Both
the library tests and the repository CI checks must pass before tagging a
release revision consumed by QuickGIFlick.
