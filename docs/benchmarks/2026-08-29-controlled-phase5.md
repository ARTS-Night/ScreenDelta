# Phase 5 controlled capture matrix — 2026-08-29

Interactive Windows desktop, 1366 x 768, release examples, ten-second GDI
stimulus. Raw CSV remains under ignored `target/bench-results`.

| Scenario / FPS | Full | Delta | Unchanged | Full payload | Delta payload | Full fallback signal |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| static / 15 | 2 | 0 | 149 | 6,291,456 B | 0 B | initial + large window paint |
| small / 15 | 13 | 138 | 0 | 40,894,464 B | 3,455,264 B | mostly empty metadata |
| typing / 15 | 13 | 138 | 0 | 40,894,464 B | 1,365,240 B | mostly empty metadata |
| scroll / 15 | 15 | 136 | 0 | 47,185,920 B | 18,435,144 B | mostly empty metadata |
| window move / 15 | 151 | 0 | 0 | 475,004,928 B | 0 B | large damage |
| full / 15 | 151 | 0 | 0 | 475,004,928 B | 0 B | large damage |
| small / 30 | 64 | 190 | 47 | 201,326,592 B | 2,646,456 B | empty metadata, one fragmented update |
| full / 30 | 231 | 1 | 69 | 726,663,168 B | 7,056 B | large damage |

All seven scenarios were run at both 15 and 30 FPS; the abbreviated table
shows the classification boundaries that drive the current policy. The raw
matrix must be used for precise CPU and timing comparisons because this desktop
session's pointer metadata and compositor activity are external variables.

## Decision

Keep the internal `<= 32 regions` and `< 50% dirty-area` Delta policy. Small,
typing, and scroll workloads use Delta frequently; window movement and full
motion safely select Full. The matrix does not justify an unmeasured rect-merge
heuristic. Empty metadata remains a Full fallback because a visible separate
pointer was reported repeatedly in this session.
