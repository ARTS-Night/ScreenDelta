# Controlled benchmark

`examples/controlled_stimulus` creates a fixed-duration GDI window workload;
it is not part of the ScreenDelta library or QuickGIFlick UI. Available
scenarios are `static`, `cursor`, `small`, `typing`, `scroll`, `window-move`,
and `full`.

Build once, then run the 15/30 FPS matrix in an interactive Windows desktop:

```powershell
cargo build --release --examples
.\tools\run-controlled-benchmark.ps1
```

The runner stores a timestamped CSV under `target\bench-results` (not source
control). It starts the stimulus first, records ScreenDelta update counts,
payload sizes, readback time, wall time, and the poll process' CPU time, then
waits for the stimulus to exit. Raw `poll_updates` text is kept in every row
so new stats can be recovered without another run.

This is a measurement fixture, not a replacement for reference-frame
correctness tests. Results compare only runs produced with the same scenario,
duration, monitor, and FPS.

The CSV also records fallback-reason, Move Rect, and pointer metadata counters.
Those counters are diagnostic evidence; they do not themselves change the
public update transport.

The first recorded matrix and its adaptive-policy decision are in
[`benchmarks/2026-08-29-controlled-phase5.md`](benchmarks/2026-08-29-controlled-phase5.md).
