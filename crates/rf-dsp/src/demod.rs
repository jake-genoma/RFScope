//! Modular analog demodulation of filtered complex baseband into normalized mono.
use num_complex::Complex32;
use std::f32::consts::{PI, TAU};

pub trait Demodulator: Send {
    fn process(&mut self, input: &[Complex32], output: &mut Vec<f32>);
}

fn valid_rate(rate: f64) -> Result<f32, &'static str> {
    if !rate.is_finite() || !(8_000.0..=2_000_000.0).contains(&rate) {
        return Err("demodulator rate must be finite and 8000–2000000 samples/s");
    }
    Ok(rate as f32)
}

/// Envelope detector normalized to a slowly tracked carrier level.
pub struct Am {
    carrier: Option<f32>,
    tracking: f32,
}
impl Am {
    pub fn new(rate: f64) -> Result<Self, &'static str> {
        Ok(Self {
            carrier: None,
            tracking: 1.0 - (-TAU * 20.0 / valid_rate(rate)?).exp(),
        })
    }
}
impl Demodulator for Am {
    fn process(&mut self, input: &[Complex32], output: &mut Vec<f32>) {
        output.clear();
        for &sample in input {
            let envelope = sample.norm();
            let carrier = self.carrier.get_or_insert(envelope);
            *carrier += self.tracking * (envelope - *carrier);
            output.push(((envelope - *carrier) / carrier.max(1e-4)).clamp(-1.0, 1.0));
        }
    }
}

struct DcBlock {
    last_input: f32,
    last_output: f32,
    feedback: f32,
}
impl DcBlock {
    fn new(rate: f32) -> Self {
        Self {
            last_input: 0.0,
            last_output: 0.0,
            feedback: (-TAU * 20.0 / rate).exp(),
        }
    }
    fn sample(&mut self, input: f32) -> f32 {
        let output = input - self.last_input + self.feedback * self.last_output;
        self.last_input = input;
        self.last_output = output;
        output
    }
}

/// Quadrature phase discriminator, normalized by the configured peak deviation.
pub struct Nfm {
    previous: Option<Complex32>,
    scale: f32,
    dc: DcBlock,
}
impl Nfm {
    pub fn new(rate: f64, deviation_hz: f32) -> Result<Self, &'static str> {
        let rate = valid_rate(rate)?;
        if !deviation_hz.is_finite() || deviation_hz <= 0.0 || deviation_hz >= rate / 2.0 {
            return Err("FM deviation must be positive and below channel Nyquist");
        }
        Ok(Self {
            previous: None,
            scale: rate / (TAU * deviation_hz),
            dc: DcBlock::new(rate),
        })
    }
}
impl Demodulator for Nfm {
    fn process(&mut self, input: &[Complex32], output: &mut Vec<f32>) {
        output.clear();
        for &sample in input {
            let value = if sample.norm_sqr() < 1e-12 {
                self.previous = None;
                0.0
            } else {
                let phase = self
                    .previous
                    .map_or(0.0, |previous| (sample * previous.conj()).arg());
                self.previous = Some(sample);
                phase * self.scale
            };
            output.push(self.dc.sample(value).clamp(-1.0, 1.0));
        }
    }
}

#[derive(Clone, Copy)]
pub enum Sideband {
    Upper,
    Lower,
}
/// Phasing SSB detector. The Hilbert filter rejects the opposite sideband;
/// I uses matching group delay. Nominal receive frequency is suppressed carrier.
pub struct Ssb {
    delay: Vec<Complex32>,
    taps: Vec<f32>,
    cursor: usize,
    sign: f32,
    dc: DcBlock,
}
impl Ssb {
    pub fn tap_count(rate: f64) -> usize {
        ((rate * 0.005).ceil() as usize / 4 * 4 + 1).clamp(129, 4097)
    }
    pub fn new(rate: f64, sideband: Sideband) -> Result<Self, &'static str> {
        let rate = valid_rate(rate)?;
        // Roughly 5 ms support; enforce a bounded odd length and even midpoint.
        let length = Self::tap_count(rate as f64);
        let middle = (length / 2) as i32;
        let taps = (0..length)
            .map(|index| {
                let n = index as i32 - middle;
                if n % 2 == 0 {
                    0.0
                } else {
                    let phase = TAU * index as f32 / (length - 1) as f32;
                    2.0 / (PI * n as f32) * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos())
                }
            })
            .collect();
        Ok(Self {
            delay: vec![Complex32::default(); length],
            taps,
            cursor: 0,
            sign: match sideband {
                Sideband::Upper => -1.0,
                Sideband::Lower => 1.0,
            },
            dc: DcBlock::new(rate),
        })
    }
}
impl Demodulator for Ssb {
    fn process(&mut self, input: &[Complex32], output: &mut Vec<f32>) {
        output.clear();
        for &sample in input {
            self.delay[self.cursor] = sample;
            let middle = (self.cursor + self.delay.len() - self.delay.len() / 2) % self.delay.len();
            let mut quadrature = 0.0;
            let mut index = self.cursor;
            for &tap in &self.taps {
                quadrature += tap * self.delay[index].im;
                index = if index == 0 {
                    self.delay.len() - 1
                } else {
                    index - 1
                };
            }
            output.push(
                self.dc
                    .sample(0.5 * (self.delay[middle].re + self.sign * quadrature))
                    .clamp(-1.0, 1.0),
            );
            self.cursor = (self.cursor + 1) % self.delay.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const RATE: f64 = 48_000.0;
    fn amplitude(samples: &[f32], frequency: f64) -> f64 {
        let mut i = 0.0;
        let mut q = 0.0;
        for (n, &value) in samples.iter().enumerate() {
            let phase = std::f64::consts::TAU * frequency * n as f64 / RATE;
            i += value as f64 * phase.cos();
            q += value as f64 * phase.sin();
        }
        2.0 * i.hypot(q) / samples.len() as f64
    }
    fn check_tone(samples: &[f32], frequency: f64, expected: f64) {
        let samples = &samples[4800..];
        assert!(samples.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        let recovered = amplitude(samples, frequency);
        assert!((recovered - expected).abs() < 0.03, "amplitude {recovered}");
        assert!(amplitude(samples, frequency + 200.0) < 0.005);
        let peak = (950..=1050)
            .map(|f| (f, amplitude(samples, f as f64)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!((peak.0 as f64 - frequency).abs() <= 1.0);
    }
    #[test]
    fn am_recovers_known_modulation_tone_and_depth() {
        let input: Vec<_> = (0..48000)
            .map(|n| {
                let modulation = (std::f64::consts::TAU * 1000.0 * n as f64 / RATE).sin() as f32;
                Complex32::new(0.4 * (1.0 + 0.5 * modulation), 0.0)
            })
            .collect();
        let mut output = Vec::new();
        Am::new(RATE).unwrap().process(&input, &mut output);
        check_tone(&output, 1000.0, 0.5);
    }
    #[test]
    fn nfm_recovers_known_deviation_and_tone_across_blocks() {
        let mut phase = 0.0;
        let input: Vec<_> = (0..48000)
            .map(|n| {
                phase += std::f64::consts::TAU
                    * 2500.0
                    * 0.6
                    * (std::f64::consts::TAU * 1000.0 * n as f64 / RATE).sin()
                    / RATE;
                Complex32::new(0.4 * phase.cos() as f32, 0.4 * phase.sin() as f32)
            })
            .collect();
        let mut demod = Nfm::new(RATE, 2500.0).unwrap();
        let mut joined = Vec::new();
        let mut chunk = Vec::new();
        for input in input.chunks(137) {
            demod.process(input, &mut chunk);
            joined.extend_from_slice(&chunk);
        }
        check_tone(&joined, 1000.0, 0.6);
        let mut whole = Vec::new();
        Nfm::new(RATE, 2500.0).unwrap().process(&input, &mut whole);
        assert_eq!(whole, joined);
    }
    #[test]
    fn ssb_recovers_selected_sideband_and_rejects_opposite() {
        for (sideband, sign) in [(Sideband::Upper, 1.0), (Sideband::Lower, -1.0)] {
            let input: Vec<_> = (0..48000)
                .map(|n| {
                    let p = std::f64::consts::TAU * 1000.0 * n as f64 / RATE;
                    let opposite = std::f64::consts::TAU * 1700.0 * n as f64 / RATE;
                    Complex32::new(
                        (0.4 * p.cos() + 0.4 * opposite.cos()) as f32,
                        (sign * (0.4 * p.sin() - 0.4 * opposite.sin())) as f32,
                    )
                })
                .collect();
            let mut output = Vec::new();
            Ssb::new(RATE, sideband)
                .unwrap()
                .process(&input, &mut output);
            check_tone(&output, 1000.0, 0.4);
            assert!(amplitude(&output[4800..], 1700.0) < 0.004);
        }
    }
    #[test]
    fn silence_and_invalid_parameters() {
        assert!(Am::new(f64::NAN).is_err());
        assert!(Nfm::new(RATE, 0.0).is_err());
        let mut output = Vec::new();
        for mut demod in [
            Box::new(Am::new(RATE).unwrap()) as Box<dyn Demodulator>,
            Box::new(Nfm::new(RATE, 2500.0).unwrap()),
            Box::new(Ssb::new(RATE, Sideband::Upper).unwrap()),
        ] {
            demod.process(&[Complex32::default(); 1024], &mut output);
            assert!(output.iter().all(|&v| v == 0.0));
        }
    }
}
