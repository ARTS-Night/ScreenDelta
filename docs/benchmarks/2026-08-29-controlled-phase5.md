# Phase 5 controlled capture matrix — 2026-08-29

Interactive Windows desktop, 1366 x 768, release examples, ten-second GDI
stimulus. Raw CSV remains under ignored `target/bench-results`.

## Pre-correction transport-policy matrix

The full seven-scenario matrix was rerun after the bounded Delta staging cache
was added. This run is `ScreenDelta_2026-08-29_22-05-45.csv`; timings are
aggregate CPU readback time for the ten-second run, not per-frame latency.

| Scenario / FPS | Full | Delta | Unchanged | Full MiB | Delta MiB | Readback ms | Staging allocs |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| static / 15 | 29 | 5 | 117 | 87.00 | 0.27 | 104.24 | 3 |
| static / 30 | 15 | 10 | 276 | 45.00 | 0.28 | 153.31 | 4 |
| cursor / 15 | 146 | 5 | 0 | 438.00 | 0.02 | 201.69 | 3 |
| cursor / 30 | 246 | 27 | 28 | 738.00 | 0.25 | 575.41 | 6 |
| small / 15 | 13 | 138 | 0 | 39.00 | 2.03 | 1.14 | 19 |
| small / 30 | 71 | 183 | 47 | 213.00 | 2.63 | 2.09 | 20 |
| typing / 15 | 15 | 136 | 0 | 45.00 | 1.29 | 951.72 | 14 |
| typing / 30 | 84 | 179 | 38 | 252.00 | 1.53 | 1.88 | 19 |
| scroll / 15 | 15 | 134 | 2 | 45.00 | 18.19 | 1.03 | 138 |
| scroll / 30 | 42 | 166 | 93 | 126.00 | 20.89 | 2.46 | 301 |
| window move / 15 | 148 | 3 | 0 | 444.00 | 0.75 | 2.01 | 3 |
| window move / 30 | 205 | 39 | 57 | 615.00 | 5.61 | 3.81 | 13 |
| full / 15 | 147 | 4 | 0 | 441.00 | 0.05 | 2.14 | 4 |
| full / 30 | 185 | 25 | 91 | 555.00 | 4.13 | 3.01 | 14 |

Small, typing, and scroll workloads commonly select Delta; window movement and
full motion select Full. The cache did not grow without bound: allocation count
is a bounded cache miss counter rather than one retained texture per region.
The typing/15 run's 951.72 ms readback outlier was not reproduced at 30 FPS;
it is retained rather than discarded.

## Pointer-only correction

The matrix exposed a correctness/performance problem: DXGI separately reported
pointer metadata while its desktop texture had no dirty rect. The old fallback
treated that as `EmptyDamage` and copied a complete desktop frame. The capture
API cannot yet compose a cursor image, so a pointer-only acquisition now emits
`Unchanged` and increments `pointer_only_updates`; move metadata remains a
Full fallback.

The exact before/after runs below use the same ten-second stimulus and release
examples. They are separate runs, so compositor activity differs; the robust
comparison is that `full_empty_damage_updates` falls to zero.

| Scenario / FPS | Before full | After full | After unchanged | After pointer-only | Before readback ms | After readback ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| cursor / 15 | 146 | 1 | 142 | 142 | 201.69 | 84.57 |
| cursor / 30 | 246 | 1 | 267 | 217 | 575.41 | 386.04 |

`static / 15` and `static / 30` also recorded zero EmptyDamage fallbacks after
the correction. The benchmark launcher now parses both `-Fps 15,30` and
space-separated values as independent rates, preventing an accidental `1530`
FPS test.

## Final regression matrix

The complete matrix was then rerun after the correction
(`ScreenDelta_2026-08-29_22-14-46.csv`). There are no `EmptyDamage` Full
fallbacks. The expected large-damage fallback remains for window movement and
full motion, while small/typing/scroll continue to prefer Delta. `Readback ms`
is aggregate CPU readback time for the ten-second run.

| Scenario / FPS | Full | Delta | Unchanged | Pointer-only | Full MiB | Delta MiB | Readback ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| static / 15 | 1 | 4 | 146 | 145 | 3.00 | 0.02 | 81.05 |
| static / 30 | 1 | 11 | 289 | 193 | 3.00 | 0.04 | 98.03 |
| cursor / 15 | 1 | 8 | 142 | 142 | 3.00 | 0.29 | 113.60 |
| cursor / 30 | 1 | 12 | 288 | 252 | 3.00 | 0.04 | 86.34 |
| small / 15 | 2 | 135 | 14 | 14 | 6.00 | 2.01 | 901.20 |
| small / 30 | 1 | 183 | 117 | 74 | 3.00 | 2.44 | 1.76 |
| typing / 15 | 1 | 134 | 16 | 16 | 3.00 | 1.29 | 691.70 |
| typing / 30 | 1 | 191 | 109 | 63 | 3.00 | 1.88 | 1.89 |
| scroll / 15 | 1 | 131 | 19 | 19 | 3.00 | 17.68 | 859.56 |
| scroll / 30 | 1 | 180 | 120 | 74 | 3.00 | 24.40 | 1.91 |
| window move / 15 | 146 | 1 | 4 | 4 | 438.00 | 0.00 | 1.40 |
| window move / 30 | 200 | 10 | 91 | 54 | 600.00 | 0.27 | 2.21 |
| full / 15 | 144 | 0 | 7 | 7 | 432.00 | 0.00 | 995.78 |
| full / 30 | 186 | 2 | 113 | 44 | 558.00 | 0.01 | 2.03 |

The 15-FPS small, typing, scroll, and full-motion readback totals are retained
as observed outliers; their 30-FPS counterparts were substantially lower.
They demonstrate host/compositor variance, not a measured causal regression.

## Decision

Keep the internal `<= 32 regions` and `< 50% dirty-area` Delta policy. Do not
add a rect-merge or GPU-comparison system without a measured bottleneck. Do
not claim cursor capture support from pointer metadata alone: cursor composition
is a separate future feature; pointer-only acquisitions are deliberately not
turned into false Full frames.
