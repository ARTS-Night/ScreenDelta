# 0003: defer Move and cursor-only updates after controlled observation

## Observation

The 2026-08-28 controlled GDI workload ran at 1366 x 768, 15 FPS.

| Scenario | Acquired | Move rects | Full | Delta | Relevant finding |
| --- | ---: | ---: | ---: | ---: | --- |
| `scroll` | 76 | 0 | 2 | 74 | normal dirty-region Delta was available |
| `window-move` | 76 | 0 | 76 | 0 | all fallback was large damage |
| `cursor` | 76 | 0 | 71 | 5 | all 76 frames reported a separate visible pointer and pointer shape metadata |

Microsoft specifies that move rectangles must be applied before dirty rectangles,
and that a visible separate pointer means the pointer is not already in the
desktop texture. See [GetFrameMoveRects](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-getframemoverects) and the [Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api).

## Decision

Do not expose `Move` or `Cursor` updates yet. The controlled workloads did not
produce Move Rect metadata, so adding a consumer API has no measured benefit.
The cursor workload proves that simply omitting cursor-damaged pixels would be
incorrect: ScreenDelta must retain and compose COLOR, MASKED_COLOR, and
MONOCHROME shapes plus old/new damage before a cursor-only transport is safe.

The capture stats retain `move_rects_observed`, pointer update, visible-pointer,
and pointer-shape counters. This makes a future workload's evidence visible
without changing the public frame contract.

## Revisit when

Add a move-aware update only after a reproducible workload yields move metadata
and a reconstructed canvas matches a Full reference. Add cursor-only only with
shape-composition correctness tests and a QuickGIFlick cursor policy consumer.
