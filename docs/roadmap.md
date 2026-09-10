# Roadmap

## Completed: milestone 1 foundation

- Rust workspace, generic IQ source, deterministic mock scene, FFT/PSD, binary v1 frames, localhost REST/WebSocket server.
- React live workstation, Canvas spectrum, WebGL2 ring-texture waterfall, controls, diagnostics, Tauri and rfscopeR scaffolds.

## Current receive-side implementation session

- Completed control slice: optional official libhackrf FFI, enumeration, selected-device open/close, queried metadata, documented board capability profiles, RX configuration, generic API/UI controls and explicit hardware diagnostic.
- Locally verified against HackRF Pro: metadata, frequency, sample rate, IF/baseband gain, filters, close/reopen. No sample reception or RF accuracy claim.
- RX implementation: bounded official callback, common IQ/FFT pipeline, start/stop/reconfiguration, diagnostics and live frontend controls. Hardware throughput/lifecycle verified; visual acceptance remains blocked by unavailable browser automation. See [validation](rx-validation.md).

## Next

Virtual-receiver channel extraction, bounded admission, API controls and spectrum
overlays are implemented; see [VFO status](vfo.md). AM/NFM/USB/LSB demodulation
is implemented and synthetically tested; see [demodulation](demodulation.md).
The fixed-rate Rust/browser audio pipeline is implemented; speaker playback remains
untested because browser automation is unavailable. See [audio](audio.md).
SigMF raw IQ recording with a bounded writer and lifecycle API is implemented; see
[recording](recording.md).
SigMF playback through the common IQ/DSP pipeline is implemented; see
[playback](playback.md).
Initial uncalibrated spectrum measurements and `/api/v1/analysis` are implemented.
rfscopeR now includes low-rate status/device/VFO/session/recording/detection/query helpers and base-R plotting functions.
Versioned SQLite metadata migrations and session/recording indexes are implemented; Parquet/DuckDB analytical storage remains outstanding.
The web shell now provides functional Live, Receivers, Recordings, Playback, Analysis, Diagnostics, Settings, Signals, and Workspaces views; persistence-backed workflows remain incremental.

1. VFO translation/filtering, AM/NFM, 48 kHz browser audio, multiple-VFO model.
2. Atomic SigMF recording/indexing and file playback through `IqSource`.
3. Signal measurements/events, SQLite + Parquet/DuckDB and useful rfscopeR queries.
4. Workstation views and persistence. Sweep and TX are out of scope for this session.

SAM, WFM/stereo, SSB/CW, triggered buffers, authentication/TLS deployment, multi-device, plugins, and RFScope Lab remain aspirational.
