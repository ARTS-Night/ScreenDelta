# 0005: Treat DXGI move rectangles as Delta candidates

## Decision

For every DXGI move rectangle, ScreenDelta now adds both its destination and
its source rectangle to the damage candidates.  Candidates that overlap are
merged before the existing Full/Delta heuristic and region readback run.

## Why

The destination contains the moved pixels, while the source contains the
newly exposed desktop.  Sending only one of them would make a Delta canvas
incorrect.  Previously any move metadata with no dirty rectangles selected a
Full fallback, even when the changed area was small.

## Safety and limits

The existing initial, fragmented (>32 regions), and large (>=50% canvas)
fallbacks remain unchanged.  This is candidate selection only: pixels still
come from the acquired texture.  The `move_damage_regions` statistic keeps
the source of this work observable in controlled captures.
