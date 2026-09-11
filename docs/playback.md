# SigMF playback

RFScope loads recordings through `rf-device::file::SigmfSource`, which implements the same `IqSource` boundary used by mock and HackRF RX. The engine therefore uses one VFO, demodulator, audio, spectrum, and waterfall path for live and recorded IQ.

The API is intentionally small for the first playback milestone:

- `GET /api/v1/playback` returns the loaded recording and sample timeline.
- `POST /api/v1/playback` with `{ "metadata_path": "/path/capture.sigmf-meta" }` validates and loads a capture.
- `PATCH /api/v1/playback` accepts `{ "action": "play" }`, `{ "action": "pause" }`, or `{ "action": "seek", "position_samples": 123 }`.
- `DELETE /api/v1/playback` ejects the source.

Only `ci8_le` SigMF captures are accepted initially. The sibling `.sigmf-data` file must contain an even number of bytes. Playback does not retune the SDR and cannot be loaded while a native hardware source is selected. The web workstation exposes a path-based load/play/pause/eject control.

The playback panel also accepts an exact sample position for seek. Seeking preserves the common downstream DSP path and resets no hardware state. It also stores named bookmarks in SQLite, linked to the loaded recording session ID; each bookmark can seek directly to its saved sample position.
