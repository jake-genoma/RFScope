# Roadmap

## Completed: milestone 1 foundation

- Rust workspace, generic IQ source, deterministic mock scene, FFT/PSD, binary v1 frames, localhost REST/WebSocket server.
- React live workstation, Canvas spectrum, WebGL2 ring-texture waterfall, controls, diagnostics, Tauri and rfscopeR scaffolds.

## Current: milestone 2

- Isolated official libhackrf FFI feature, enumeration/open/RX/configuration, dynamic capabilities and opt-in hardware tests.

## Next

1. VFO translation/filtering, AM/NFM, 48 kHz browser audio, multiple-VFO model.
2. Atomic SigMF recording/indexing and file playback through `IqSource`.
3. Signal measurements/events, SQLite + Parquet/DuckDB and useful rfscopeR queries.
4. Capability-driven time-multiplexed HackRF sweep.

SAM, WFM/stereo, SSB/CW, triggered buffers, authentication/TLS deployment, multi-device, plugins, and RFScope Lab remain aspirational.
