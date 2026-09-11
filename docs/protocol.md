# API and spectrum protocol

Virtual-receiver endpoints and payloads are documented in [VFO API](vfo.md).
Demodulator output diagnostics are described in [demodulation](demodulation.md).
PCM audio framing and its WebSocket route are documented in [audio](audio.md).
SigMF recording lifecycle and metadata are documented in [recording](recording.md).

## REST v1

- `GET /api/v1/status` and `GET /api/v1/device/state`: version, source, current state, FFT size, diagnostics.
- `PATCH /api/v1/device/state`: optional JSON fields `center_frequency_hz`, `sample_rate_hz`, `running`, `fft_size`; invalid rates/sizes return HTTP 400.
- `GET /api/v1/metrics`: diagnostics snapshot.
- `GET /api/v1/stream/spectrum`: WebSocket upgraded to binary spectrum frames.

Commands, snapshots, diagnostics, and continuous streams are separate. Additional device/VFO/recording routes are roadmap items, not fictional endpoints.

## Binary spectrum frame v1

All integers and IEEE-754 floats are **little-endian**. Header is exactly 48 bytes.

| Offset | Type | Meaning |
|---:|---|---|
| 0 | 4 bytes | ASCII `RFSP` |
| 4 | u16 | protocol version = 1 |
| 6 | u16 | stream type = 1 (PSD) |
| 8 | u32 | header bytes = 48 |
| 12 | u64 | sequence number |
| 20 | u64 | Unix timestamp, nanoseconds |
| 28 | u64 | center frequency Hz |
| 36 | u32 | sample rate Hz |
| 40 | u32 | FFT/bin count |
| 44 | u32 | flags (zero in v1) |
| 48 | `bin_count` × f32 | frequency-ordered dBFS bins |

Unknown versions/types must be rejected. Later audio/baseband/preview formats receive distinct stream types and documentation rather than overloading v1.

## Device ownership and receiver control

Hardware support is opt-in (`rf-server --features hackrf`). Existing status and
state routes remain available; status adds `device` with `descriptor`,
`capabilities`, string-valued `metadata`, `opened`, `supports_iq_streaming`, and
nullable `configuration`, and `running`. `source` follows the selected driver. Hardware `state.running` reports the owner’s RX state. `opened` means
exclusive control ownership, not reception; closed hardware reports zero
frequency/rate in `state` and null applied configuration.

- `GET /api/v1/devices`: `{devices: DeviceDescriptor[], warnings: string[]}`;
  includes mock even when native discovery fails or the feature is disabled.
- `GET /api/v1/device/control`: current `DeviceSelection`.
- `PATCH /api/v1/device/control`: tagged JSON command, returning full status:
  - `{"action":"select","id":"hackrf:<full serial>"}` or ID `mock-0`.
  - `{"action":"open"}` / `{"action":"close"}` (idempotent for hardware).
  - `{"action":"start"}` / `{"action":"stop"}`: receive-only start/stop, retaining ownership on stop.
  - `{"action":"configure","configuration":{"center_frequency_hz":100000000,"sample_rate_hz":8000000,"gains":{"if":16,"baseband":20,"rf_amp":0},"baseband_filter_bandwidth_hz":5000000}}`.

Configure supplies every reported gain stage. Stage IDs/labels/units/ranges and
filter choices come from capabilities. The example IDs are backend-reported,
not generic required names. Filter bandwidth must not exceed sample rate.
Unknown fields are rejected. Selecting another device while one is owned returns
409; close first. Hardware selection pauses mock production (including a failed
selection attempt); returning to mock requires explicit START RX. Selection alone
does not open hardware, so capabilities are placeholders until open queries the
board. Check `opened` before rendering controls.

`PATCH /api/v1/device/state` also configures selected, opened hardware: optional
frequency, sample rate, `gains` (complete map) and
`baseband_filter_bandwidth_hz` fields merge with the accepted configuration.
Hardware accepts `running` and `fft_size`; ownership still uses open/close. Configuration during RX restarts capture with a fresh source.
Mock retains its existing controls and rejects hardware gain/filter fields.
Validation happens before shared mock state changes or native setters run.

Invalid settings return 400, ownership conflicts 409, native/unavailable/queue
errors 503 with operation and native code/name when available. A partial native
configuration failure closes the device and clears accepted settings; fetch status
after errors. Configuration is accepted settings, not readback. Binary spectrum v1 is unchanged and carries either live or mock PSD. WebSocket clients can disconnect while
mock is paused; lagging clients skip missed frames instead of terminating.

## RX diagnostics

`received_samples` counts complex samples consumed by DSP over the engine lifetime.
`received_bytes`, `received_blocks`, `dropped_iq_blocks`, `dropped_iq_bytes`,
`invalid_iq_blocks`, `stream_faults`, and `hardware_streaming` describe the latest
RX source and reset on RX restart. Native bytes include application-dropped bytes;
complex samples equal bytes / 2 for signed interleaved IQ. Invalid transfer lengths
are counted separately. Expected stop/reconfigure gaps and intentionally discarded
queued blocks are not overload drops. Counters cannot detect firmware/USB loss.
`fft_frames` and display/client metrics remain engine-lifetime counters. Display
no-subscriber drops do not mean IQ loss. Health is polled on the owner at 100 ms.
