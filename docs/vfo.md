# Virtual receivers

VFOs are software channels within the selected capture. Their stable session IDs,
names, absolute frequency, mode, nominal bandwidth, squelch, AGC, volume, mute and
solo settings live in a low-rate registry. Recording state is read-only and false
until receiver audio recording exists. AM/NFM/USB/LSB modes now select modular
demodulators after channel extraction. See [demodulation](demodulation.md).
Browser audio playback follows separately.

`VfoProcessor` belongs to the DSP thread. Each receiver translates IQ with a
persistent complex oscillator, passes it through cascaded 31-tap Blackman FIR
decimators, and applies a final Blackman channel FIR. The final filter length
scales with bandwidth (129–4,095 taps). Decimation stops at a rate at least
48 ksample/s and four times channel bandwidth. Output rate is capture rate divided
by a power of two, not yet fixed 48 kHz. Phase and delay lines persist across blocks;
frequency/mode/bandwidth/capture changes and reported IQ drops reset them.

Requested nominal passbands must lie entirely inside instantaneous capture
bandwidth. Bandwidth is 500–100,000 Hz, volume 0–2, squelch null or -120–0 dBFS,
and names 1–128 bytes. Hardware changes can move an existing channel out of band;
it is suspended with a reason, without retuning the device. It resumes if later
capture settings contain it. Finite FIR transition bands mean nominal bandwidth
is not a brick-wall cutoff, and reported power remains uncalibrated dBFS.

Admission is bounded by a conservative estimated tap-work budget of 1 billion
units/second across receivers, rather than a fixed receiver count. `VfoBank::new`
accepts a budget for embedders/tests. Capture changes re-evaluate the budget and
suspend excess channels in stable ID order. This bounds allocations and intended
work, not actual wall-clock cost; the raw queue still exposes overload. A test
creates twelve receivers at a low capture rate to reject a ten-receiver assumption.

The frontend offers add/remove, frequency, mode and bandwidth controls. Spectrum
overlays show channel width and label in the renderer, outside React pixel updates.
Visual acceptance remains unverified because no browser was available in this
session. Numeric controls are the initial tuning interaction.

## API

- `GET /api/v1/vfos` returns receiver snapshots, including output rate, cumulative
  channel samples, channel power and nullable `suspended_reason`.
- `POST /api/v1/vfos` accepts a full configuration and returns the new receiver.
- `PATCH /api/v1/vfos/{id}` replaces the full configuration, retaining ID.
- `DELETE /api/v1/vfos/{id}` removes it and returns 204; missing IDs return 404.

Configuration example:

```json
{"name":"Center AM","frequency_hz":100000000,"mode":"am","bandwidth_hz":10000,"squelch_dbfs":null,"agc":true,"volume":1,"mute":false,"solo":false}
```

Invalid configurations/budget exhaustion return 400 without mutating the registry.
No VFO endpoint invokes hardware configuration. The `recording` field cannot be
set by clients. Receiver persistence is a later milestone.

## Verification

Deterministic tests recover a +1 kHz tone after +300 kHz translation at 2 MS/s,
check amplitude and 62.5 ksample/s output, reject a tested adjacent carrier by
over 40 dB, and compare split versus contiguous input exactly. Tests also cover
capture/budget validation, stable IDs, hardware-window suspension, and API edits
that preserve capture settings. Frontend tests verify RF-to-overlay coordinates.

Initial single-channel release throughput on the documented RX host measured
21.23 MS/s (2 MS/s configuration) and 23.23 MS/s (20 MS/s configuration), for a
12 kHz channel. This is a microbenchmark, not a sustained multi-VFO guarantee.

The explicit hardware API check ran two receivers at 8 MS/s on the same HackRF
Pro documented for RX. Both produced 94,208 channel samples at 62.5 ksample/s.
Changing one to 100.05 MHz, NFM and 12 kHz bandwidth retained the identical
hardware configuration and monotonically increasing native byte counters.
At the snapshot, 39,845,888 native bytes had arrived with zero IQ drops or stream
faults. The complete check received 77 valid spectrum frames and restored mock.
This short run does not establish sustained capacity. Default/feature-enabled
Rust tests passed (16/17), both Clippy configurations passed with warnings denied,
and formatting, frontend tests (2), typecheck, build, and Rust builds passed.
