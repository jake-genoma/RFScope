# API and spectrum protocol

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
