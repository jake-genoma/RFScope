# Roadmap

## Completed: milestone 1 foundation

- Rust workspace, generic IQ source, deterministic mock scene, FFT/PSD, binary v1 frames, localhost REST/WebSocket server.
- React live workstation, Canvas spectrum, WebGL2 ring-texture waterfall, controls, diagnostics, Tauri and rfscopeR scaffolds.

## Current: milestone 2

- Completed control slice: optional official libhackrf FFI, enumeration, selected-device open/close, queried metadata, documented board capability profiles, RX configuration, generic API/UI controls and explicit hardware diagnostic.
- Locally verified against HackRF Pro: metadata, frequency, sample rate, IF/baseband gain, filters, close/reopen. No sample reception or RF accuracy claim.
- Next narrow slice: bounded RX callback ingestion through the shared IQ/FFT path, stop/disconnect handling, and hardware sample-integrity checks. Hardware streaming remains unimplemented.

## Next

1. VFO translation/filtering, AM/NFM, 48 kHz browser audio, multiple-VFO model.
2. Atomic SigMF recording/indexing and file playback through `IqSource`.
3. Signal measurements/events, SQLite + Parquet/DuckDB and useful rfscopeR queries.
4. Capability-driven time-multiplexed HackRF sweep.

SAM, WFM/stereo, SSB/CW, triggered buffers, authentication/TLS deployment, multi-device, plugins, and RFScope Lab remain aspirational.
