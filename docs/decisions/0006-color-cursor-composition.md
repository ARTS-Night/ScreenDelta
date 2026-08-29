# 0006: Compose supported DXGI Color cursor shapes in ScreenDelta

## Decision

`CursorCapture::Include` retains the latest DXGI Color pointer shape and
composites it after CPU readback. Pointer movement adds the previous and new
cursor bounds as Delta candidates, so the exposed background and new pointer
are both represented. `Exclude` preserves the existing pointer-only
`Unchanged` transport behavior.

## Why

Desktop Duplication reports pointer metadata separately from desktop texture
pixels. QuickGIFlick needs an Original cursor recording without forcing full
desktop transfers. Keeping this in ScreenDelta preserves the generic capture
boundary and lets every consumer choose Include or Exclude.

## Scope

Only DXGI Color shapes are composited. Monochrome and Masked Color shapes are
excluded safely until their mask semantics have dedicated compatibility tests.
The blend function and old/new damage semantics have unit coverage.

`CursorCapture::System` is a separate best-effort path for applications that
want Windows-standard cursor rendering. It maps known Arrow, Hand, I-Beam,
Resize, Busy, and Wait handles to a top-down DIB and falls back to Arrow when
the current handle is not recognized.

## Windows observation

On 2026-08-30, the 1366×768 cursor stimulus at 15 FPS produced 44 Delta
updates and 46 composited cursor updates in three seconds with Include. The
same Hidden/Exclude capture produced 36–37 pointer-only Unchanged updates.
The Standard/System run produced 44 Delta updates and 46 composites, with
Full and Partial GIFs both decoding to 3.01 seconds.
