# Pointer-only desktop-duplication updates are unchanged pixels

## Context

`IDXGIOutputDuplication::AcquireNextFrame` can report changed pointer metadata
without a desktop dirty rectangle. The acquired desktop texture does not itself
contain a cursor image. The initial Delta pipeline classified every empty-dirty
acquisition as a Full fallback, causing repeated full readbacks while a cursor
moved on an otherwise static desktop.

## Decision

When there are neither relevant dirty rectangles nor DXGI move rectangles,
ScreenDelta emits `CaptureUpdate::Unchanged`. If pointer metadata changed, it
also increments `CaptureStats::pointer_only_updates`. Move rectangles continue
to select Full because the public update model does not yet express a move.

## Consequences

This removes a false full-frame transfer and makes the statistic visible in
controlled benchmarks. It intentionally does not pretend to capture a cursor:
an opt-in cursor-composition API must provide real cursor pixels and damage for
both the old and new cursor locations before it can be exposed as a Delta.
