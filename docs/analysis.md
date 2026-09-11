# Initial RF analysis

The DSP crate exposes deterministic measurements over FFT-shifted PSD bins: peak frequency and dBFS, median noise floor, peak-minus-noise SNR, -3/-6 dB occupied spans, a 99% power span, and amplitude statistics. Values are explicitly uncalibrated dBFS unless a future calibration profile is applied.

The latest measurement is available at `GET /api/v1/analysis`. It is updated alongside spectrum frames and is intentionally a latest-value endpoint; historical observations and event duration tracking belong in the persistence milestone.

Signal markers are structured records with stable IDs, frequency, label, and color. They are managed through `GET/POST /api/v1/markers` and `DELETE /api/v1/markers/{id}`. Event detection and durable annotation storage remain follow-up work.

The engine now turns measurements above 12 dB peak-minus-median SNR into bounded events. Three consecutive below-threshold frames close an event and assign its end time, so duration is available whenever an event is complete. `GET /api/v1/detections` returns active and completed events. These are uncalibrated measurement events, not modulation classifications.
