# Initial RF analysis

The DSP crate exposes deterministic measurements over FFT-shifted PSD bins: peak frequency and dBFS, median noise floor, peak-minus-noise SNR, -3/-6 dB occupied spans, a 99% power span, and amplitude statistics. Values are explicitly uncalibrated dBFS unless a future calibration profile is applied.

The latest measurement is available at `GET /api/v1/analysis`. It is updated alongside spectrum frames and is intentionally a latest-value endpoint; historical observations and event duration tracking belong in the persistence milestone.
