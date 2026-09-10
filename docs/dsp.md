# DSP

The source API yields normalized `Complex32` IQ. The HackRF backend preserves interleaved signed 8-bit raw IQ until conversion is needed. The demo generates seeded noise plus CW at −300 kHz, 1 kHz-tone AM at zero offset, and tone-modulated NFM at +400 kHz.

The PSD stage supports power-of-two FFT sizes and rectangular, Hann, Hamming, Blackman, and Blackman-Harris windows. Output is FFT-shifted into ascending baseband frequency. Magnitude is normalized for FFT length and coherent window gain and clamped before `10 log10(power)`. Values are uncalibrated **dBFS**, never dBm. Device/frequency calibration corrections will be a separate future abstraction.

VFO processing now implements digital translation, FIR filtering and staged
decimation. See [VFO design and tests](vfo.md). Modular demodulators, squelch and
AGC behind a 48 kHz audio boundary are the next stages. AM/NFM/USB/LSB
demodulators now operate on channel output; see [validation](demodulation.md).

`IqSource::read` returns the valid sample count. Hardware transfers normalize signed bytes by 128 outside the callback. The dedicated engine drains all blocks and selects the tail FFT window of eligible blocks for display; this is a visualization sampler, not full-band continuous detection. FFT plans, window and IQ buffers are reused.
