# Analog demodulation

AM, NFM, USB and LSB implement the `Demodulator` trait in `rf-dsp`. Input is
filtered, decimated complex baseband from the common VFO channelizer. Output is
one normalized mono `f32` sample per input sample, bounded to -1…1. Demodulator
state persists across blocks and resets with channel tuning, mode, capture or
reported IQ discontinuities. No audio is produced in the native callback.

AM measures the envelope and subtracts a carrier estimate tracked at 20 Hz. The
result is normalized by that carrier estimate, with a small floor. This recovers
modulation depth for ordinary non-overmodulated signals; it is not synchronous AM.
Noise without a carrier can sound loud until the following squelch/audio stage.

NFM uses the phase of the product of each sample and the conjugate of its
predecessor. The discriminator is scaled by sample rate and nominal deviation,
then DC-blocked at 20 Hz. VFO nominal deviation is bandwidth/5, limited to
500–5,000 Hz. This is an initial normalization policy, not detected modulation
deviation; an explicit expert control can follow. Zero-magnitude input resets
phase history. NFM is mono without stereo or de-emphasis in this milestone.

SSB uses a windowed Hilbert FIR on Q and matched delay on I. The selected sum or
difference rejects the opposite sideband. The filter has approximately 5 ms
support (129–4,097 taps), and its work enters receiver admission estimates.
The VFO's frequency is the suppressed carrier. Bandwidth continues to mean the
total symmetric complex channel width: use 6 kHz for about 0–3 kHz selected-sideband
coverage. There is finite rejection near DC and finite channel transition width.

`GET /api/v1/vfos` adds `demodulated_samples` and `demodulated_peak`. These measure
the demodulator output before user audio processing. The frontend displays them;
browser playback and the fixed 48 kHz boundary are the next milestone. Neither
the peak nor channel power is calibrated RF power.

## Deterministic validation

- AM: 1 kHz, 50% depth, recovered amplitude within 0.03 and frequency within 1 Hz.
- NFM: 1 kHz, 2.5 kHz nominal deviation, 60% peak modulation; recovered amplitude
  within 0.03 and frequency within 1 Hz. Split/continuous output compares exactly.
- USB and LSB: selected 1 kHz tone recovered at amplitude 0.4 ±0.03; equal-amplitude
  opposite-sideband 1.7 kHz tone suppressed below 0.004 (over 40 dB rejection).
- Silence and invalid-rate/deviation tests; finite normalized output checks.
- Shared pipeline: 200 ksample/s IQ at a 30 kHz carrier offset passes through
  translation/filtering/decimation into AM or NFM. Both recover 1 kHz at amplitude
  0.5 ±0.03, producing 12,500 audio samples from 50,000 IQ samples.

No subjective listening result is claimed. The synthetic tests establish the
specific cases above, not arbitrary interference tolerance or RF calibration.

Default and hardware-feature Rust tests passed (21/22), both Clippy configurations
passed with warnings denied, and formatting, frontend tests (2), typecheck/build,
and Rust default/release builds passed. The live API regression on the HackRF Pro
at 8 MS/s produced 95,232 NFM and 93,184 AM samples, finite bounded peaks, zero
application IQ drops and zero stream faults at the sampled snapshot. Peaks reached
the limiter on weak/noise-level input; no known RF tone or listening was tested.
The script received 78 valid spectrum frames and returned to mock.

The release microbenchmark at 62.5 ksample/s settings measured AM 185.25 MS/s,
NFM 139.54 MS/s, USB 2.69 MS/s and LSB 2.73 MS/s using constant synthetic IQ.
These are local single-run timings; signal-dependent cost and sustained concurrent
processing require further measurement.
