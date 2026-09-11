# Audio pipeline

Each VFO demodulator output is anti-aliased and resampled to exactly 48,000 mono
floating-point samples per second. High-pass and low-pass hooks default to 80 Hz
and 5 kHz, with validated ranges of 0–1 kHz and 500–12 kHz. Per-VFO volume,
mute, solo and AGC are applied before output. Squelch uses uncalibrated channel
dBFS, 3 dB hysteresis and a 100 ms hold. Samples are bounded to -1…1.

Rust publishes fixed 960-sample (20 ms) packets on a broadcast bus of capacity 32.
The `RFAU` version 1, stream type 2 frame has a 48-byte little-endian header,
sequence, source epoch, 48 kHz rate, count, VFO ID length, flags, timestamp, UTF-8
ID and f32 samples. The epoch changes when channel processing is rebuilt. Raw PCM
is an explicit seam for a later Opus/WebRTC transport. DSP packetization never
blocks IQ ingestion; lagged audio clients resubscribe and increment diagnostics.

The browser AudioWorklet owns a fixed 7,680-sample queue per VFO, primes at 60 ms,
interpolates at the output device rate, and mixes active queues. The main thread
allows eight unacknowledged packets; excess frames count as drops. Health reports
underruns, overruns and sequence/epoch discontinuities. AudioContext startup needs
a user gesture. Browser speaker playback is untested because no browser was
available in this session; the mock integration test delivered 32 valid packets.

`GET /api/v1/stream/audio` is a multiplexed PCM WebSocket keyed by VFO ID.
VFO configuration accepts `audio_highpass_hz` and `audio_lowpass_hz`, defaulting
to 80 and 5,000. `Diagnostics` reports audio clients and lagged frames; VFO
snapshots report audio rate, samples, frames, peak, squelch and active state.

Deterministic tests cover 48 kHz output at 48, 62.5, 78.125, 100 and 400 ksample/s,
block continuity, alias rejection, volume/mute/squelch/AGC, normalized samples,
packet dimensions and fixed transport capacity. Vitest covers queue priming,
bounded storage, overruns and discontinuities. The mock run produced 1,852,320
audio samples and 1,929 packets for one AM VFO, with 48 kHz and 0.213 peak. The
reproducible `scripts/audio-api-check.mjs` performs the same check against a
running mock server; its final 1.5-second run received 23 valid frames and all
payload samples were finite and within range. Counters are cumulative per VFO, so
the long-lived test server reported 6,467,520 audio samples and 6,737 frames at
that point. The final live HackRF regression reported two VFOs at 48 kHz with
73,137 and 71,565 audio samples, 75 and 74 PCM frames, finite peaks of 0.308 and
0.398, zero IQ drops and zero stream faults. No speaker output was claimed.
