# Initial RF analysis

The DSP crate exposes deterministic measurements over FFT-shifted PSD bins: peak frequency and dBFS, median noise floor, peak-minus-noise SNR, -3/-6 dB occupied spans, a 99% power span, and amplitude statistics. Values are explicitly uncalibrated dBFS unless a future calibration profile is applied.

The latest measurement is available at `GET /api/v1/analysis`. It is updated alongside spectrum frames and is intentionally a latest-value endpoint; historical observations and durable event records belong in the persistence layer.

Signal markers are structured records with stable IDs, frequency, label, and color. They are managed through `GET/POST /api/v1/markers` and `DELETE /api/v1/markers/{id}`. Event detection and durable annotation storage remain follow-up work.

The engine now turns measurements above 12 dB peak-minus-median SNR into bounded events. Three consecutive below-threshold frames close an event and assign its end time, so duration is available whenever an event is complete. Completed events enter a bounded asynchronous SQLite writer; `GET /api/v1/detections` returns the latest persisted events together with active in-memory events. These are uncalibrated measurement events, not modulation classifications.

`GET /api/v1/analysis/offsets` reports the current peak frequency offset in Hz relative to every active marker and VFO. These are derived from the current FFT measurement and receiver configuration, so they are uncalibrated and are intentionally low-rate control-plane data.
