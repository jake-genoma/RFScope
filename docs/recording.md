# SigMF recording

The receive path can tee raw signed interleaved HackRF IQ into SigMF without
blocking the native callback. `RecordingSink` owns a preallocated pool of 32
262,144-byte blocks and a 16-block synchronous writer queue. The callback only
tries the pool lock, copies bytes, and attempts a nonblocking queue send. A
dedicated writer thread performs file writes, returns blocks to the pool, and
updates counters. IQ ingestion and display remain independent.

Each start creates `capture-<unique-id>/<id>.sigmf-data` and `.sigmf-meta` below
`RFSCOPE_RECORDINGS` (default `recordings`). Data uses SigMF `ci8_le`: byte pairs
are signed I and Q, so samples written equals bytes divided by two. Metadata
includes SigMF core version, datatype, sample rate, center frequency, hardware
descriptor, RFScope recorder/session ID, capture start timestamp and a capture
record. Files use create-new semantics; an existing directory or filename never
gets silently overwritten. `validate_sigmf` checks datatype, nonzero frequency and
rate, capture presence, and even data length.

The writer reports elapsed time, bytes/samples written, current queue depth,
projected bytes/s (2 × sample rate), available disk space, dropped blocks/bytes,
data write errors, and metadata write errors. Disk space is sampled only while
serving status, never on the callback or writer path. A recording stop returns an
explicit error if any blocks or metadata writes failed; it does not claim complete
data. Successful device-state changes append a `device_settings_changed` SigMF
annotation with the applied state and requested control patch. Frequency or
sample-rate changes also append a capture boundary at the current written sample.
This low-rate metadata work runs on the control path, never in the callback.

## API and UI

- `GET /api/v1/recording` returns the active summary or `null`.
- `POST /api/v1/recording` starts one recording from the selected capture's
  accepted frequency/rate and device descriptor. Starting twice returns 400.
- `DELETE /api/v1/recording` stops and finalizes the writer. A drop/write failure
  returns 503 with the diagnostic.

The Live view exposes Start IQ recording/Stop recording, elapsed time, bytes,
queue depth, drop count and projected storage rate. Recording requires RX to be
running. Selecting/closing a device stops RX but does not silently finalize an
active recording; callers should stop first. The writer is receive-only; no TX
symbols or files exist.

## Verification

Unit tests use temporary directories to verify unique sessions, create-new
semantics, exact bytes/samples, metadata validation, bounded overload and clean
finalization. The live HackRF Pro check on 2026-09-10 ran at 8 MS/s for 1.207 s:
18,874,368 bytes and 9,437,184 samples were written, with zero drops and zero
write errors. Metadata matched `ci8_le`, 8 MS/s and the accepted center frequency.
The final test wrote under `/tmp/rfscope-capture-test`; no repository data was
created. On 2026-09-11, the HackRF Pro was recorded at 2 MS/s for 8.054 seconds:
32,243,712 bytes / 16,121,856 samples, zero dropped blocks, and zero data or
metadata write errors. A 100 kHz retune created a capture boundary at sample
9,306,112 and a `device_settings_changed` annotation. Long-duration disk
throughput, disk-full behavior, and unplug during a recording remain untested.
