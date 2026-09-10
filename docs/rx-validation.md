# Receive streaming validation — 2026-09-10

Milestone 1 implementation and hardware/API validation are complete. Its visual
acceptance criterion is **not verified**: computer-use inventory returned no
browsers, and creating the in-app browser returned `Browser is not available: iab`.
The frontend uses the existing RFSP renderer for live frames, but a person still
needs to observe spectrum and waterfall with the physical device selected.

## Physical device and measured throughput

HackRF Pro r1.2, firmware n_260808, library 2026.01.3 / API 0.9.2, serial
0000000000000000977c64de21718a13. Linux 7.1.12-200.fc44.x86_64, Intel i5-10210U.
Release build; 2,048-bin Blackman-Harris FFT; all gains zero, RF amplifier and
antenna power disabled. The explicit diagnostic measured each rate for about
15 seconds after a 300 ms warmup, with a 1.75 MHz filter at 2 MS/s and a 5 MHz
filter for the other rates. It measures host callback throughput, not RF accuracy.

| Requested MS/s | Observed native MS/s | Consumed MS/s | Dropped blocks | Dropped bytes | FFT frames/s |
|---:|---:|---:|---:|---:|---:|
| 2 | 2.0022 | 2.0022 | 0 | 0 | 15.28 |
| 8 | 8.0008 | 8.0008 | 0 | 0 | 20.37 |
| 10 | 10.0026 | 10.0026 | 0 | 0 | 19.99 |
| 20 | 20.0069 | 19.9982 | 1 | 262,144 | 22.62 |

20 MS/s is not established as lossless. Application drops include pool exhaustion
and lock contention; the current aggregate counter does not distinguish them.
The callback cannot detect samples lost before delivery by firmware/USB. The
full-transfer cadence and display throttle explain FFT rates below 25 FPS.

At every rate, active retune/reconfiguration, idempotent stop, and resumed RX
passed. Five additional start/stop cycles, close while running, reopen, and
explicit library shutdown passed. Physical unplug, long-duration reliability,
RF amplitude/frequency calibration and other boards remain untested.

## Reproducible checks

`cargo run -p rf-server --release --features hackrf --example rx-diagnostic -- SERIAL 15`
runs the receive-only lifecycle/throughput test. Ordinary Cargo tests do not open
USB. A restricted sandbox could not initialize libhackrf; the test ran with host
USB access. An older idle RFScope instance owned the receiver and was closed via
its documented API before testing. No process was killed or USB settings changed.

`node scripts/rx-api-check.mjs` tested an isolated release server on localhost:8788.
It received 46 valid, finite RFSP frames labelled 100 MHz/8 MS/s, 101 MHz/8 MS/s,
and 101 MHz/10 MS/s; invalid-rate rejection preserved running configuration.
Stop silenced frames, restart resumed them, close released the device, and the
script returned the server to mock. The final RX source had zero dropped blocks
and zero stream faults. This is protocol validation, not visual browser testing.

Software checks include default and feature-enabled workspace tests and Clippy,
Cargo formatting, default/release server builds, and frontend test/typecheck/build.
Tests cover bounded overload, callback contention, signed-byte conversion, pool
recycling, stopped-source behavior, and a deterministic tone through the shared
IQ/FFT encoder. See the session report for final check counts and commits.

## Initial microbenchmark

`cargo run -p rf-engine --release --example pipeline-bench` measured bounded copy
plus signed conversion at 533.14 MS/s, with mean FFT times of 30.57 µs (2,048),
125.23 µs (8,192), and 1,097.68 µs (65,536). These are a single local run, not a
statistical regression baseline. The planned Criterion suite remains follow-up
work; this small dependency-free harness is the initial measurement instead.
