# DSP

The source API yields normalized `Complex32` IQ. The HackRF backend will preserve interleaved signed 8-bit raw IQ until conversion is needed. The demo generates seeded noise plus CW at −300 kHz, 1 kHz-tone AM at zero offset, and tone-modulated NFM at +400 kHz.

The PSD stage supports power-of-two FFT sizes and rectangular, Hann, Hamming, Blackman, and Blackman-Harris windows. Output is FFT-shifted into ascending baseband frequency. Magnitude is normalized for FFT length and coherent window gain and clamped before `10 log10(power)`. Values are uncalibrated **dBFS**, never dBm. Device/frequency calibration corrections will be a separate future abstraction.

VFO processing will add digital translation, FIR filtering, decimation/resampling, modular demodulators, squelch and AGC behind a 48 kHz audio boundary. AM and NFM are milestone 3 priorities.
