# Performance design

The visualization broadcast is bounded at four frames and targets 25 FPS. Display delivery is lossy by design; stale frames do not impair source health. Metrics expose samples, generated frames, dropped/no-subscriber frames, and connected clients. Recording will receive a higher-priority independent bounded path and explicitly surface disk throughput failures.

The demo favors clarity over benchmark claims. Initial conversion and FFT timing
uses a dependency-free release harness; actual hardware rates and limitations are
recorded in [RX validation](rx-validation.md). Add statistical Criterion benchmarks
for conversion, FFT, translation, FIR, demodulation, resampling and encoding as
those stages mature. The current results are not a regression baseline.
