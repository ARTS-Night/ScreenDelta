# 0004: Reuse a bounded Delta staging-texture cache

## Context

Delta delivery reads each verified dirty region through a D3D11 staging
texture. The original implementation created a staging texture for every
region, even when many updates repeated the same small sizes. That adds D3D
object allocation to the capture hot path.

## Measurement

Windows controlled `small` stimulus, 10 seconds, 15 FPS, release build:

| Delta updates | Delta regions | Delta staging allocations | Delta bytes | readback time |
| ---: | ---: | ---: | ---: | ---: |
| 143 | 202 | 13 | 2,163,488 B | 938.4942 ms |

The prior code allocated once per region by construction, so the same run's
202 region readbacks would have requested up to 202 staging textures. This is
an implementation-path comparison, not a claim that total readback time has
improved; the test was measured only after the cache was added.

## Decision

Keep at most eight staging textures keyed by exact width and height. Reuse is
safe because each region is mapped and unmapped synchronously before the next
readback. On a cache miss, the cache evicts round-robin instead of retaining
unbounded region sizes. Full-frame staging remains separate and unchanged.

The `delta_staging_allocations` development statistic makes future workload
regressions visible without making cache internals part of the public update
API.
