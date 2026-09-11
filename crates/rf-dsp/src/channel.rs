//! Streaming channel extraction: NCO translation, anti-alias FIR stages, channel FIR.
use num_complex::Complex32;
use std::f64::consts::TAU;

fn channel_taps(rate: f64, bandwidth: u32) -> usize {
    ((16.0 * rate / bandwidth as f64).ceil() as usize | 1).clamp(129, 4095)
}

struct Fir {
    taps: Vec<f32>,
    delay: Vec<Complex32>,
    cursor: usize,
    phase: usize,
    decimation: usize,
}
impl Fir {
    fn new(length: usize, cutoff: f64, decimation: usize) -> Self {
        let middle = (length - 1) as f64 / 2.0;
        let mut taps: Vec<f32> = (0..length)
            .map(|n| {
                let x = n as f64 - middle;
                let sinc = if x == 0.0 {
                    2.0 * cutoff
                } else {
                    (TAU * cutoff * x).sin() / (std::f64::consts::PI * x)
                };
                let phase = TAU * n as f64 / (length - 1) as f64;
                (sinc * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos())) as f32
            })
            .collect();
        let gain: f32 = taps.iter().sum();
        for tap in &mut taps {
            *tap /= gain;
        }
        Self {
            taps,
            delay: vec![Complex32::default(); length],
            cursor: 0,
            phase: 0,
            decimation,
        }
    }
    fn push(&mut self, sample: Complex32) -> Option<Complex32> {
        self.delay[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.delay.len();
        self.phase += 1;
        if self.phase < self.decimation {
            return None;
        }
        self.phase = 0;
        let mut sum = Complex32::default();
        let mut index = self.cursor;
        for &tap in &self.taps {
            index = if index == 0 {
                self.delay.len() - 1
            } else {
                index - 1
            };
            sum += self.delay[index] * tap;
        }
        Some(sum)
    }
}

pub struct Channelizer {
    oscillator: num_complex::Complex64,
    step: num_complex::Complex64,
    phase_count: u32,
    stages: Vec<Fir>,
    filter: Fir,
    output_rate: f64,
}
impl Channelizer {
    pub fn new(sample_rate: u32, offset_hz: f64, bandwidth_hz: u32) -> Result<Self, &'static str> {
        if sample_rate == 0
            || !offset_hz.is_finite()
            || bandwidth_hz == 0
            || offset_hz.abs() + bandwidth_hz as f64 / 2.0 > sample_rate as f64 / 2.0
        {
            return Err("channel passband must fit inside captured IQ");
        }
        let mut output_rate = sample_rate as f64;
        let mut stages = Vec::new();
        // Keep the wanted band well inside every decimation filter's passband.
        while output_rate / 2.0 >= (bandwidth_hz as f64 * 4.0).max(48_000.0) {
            stages.push(Fir::new(31, 0.20, 2));
            output_rate /= 2.0;
        }
        Ok(Self {
            oscillator: num_complex::Complex64::new(1.0, 0.0),
            step: num_complex::Complex64::from_polar(1.0, -TAU * offset_hz / sample_rate as f64),
            phase_count: 0,
            stages,
            filter: Fir::new(
                channel_taps(output_rate, bandwidth_hz),
                bandwidth_hz as f64 / 2.0 / output_rate,
                1,
            ),
            output_rate,
        })
    }
    pub fn output_rate(&self) -> f64 {
        self.output_rate
    }
    /// Conservative tap-work estimate, used for admission rather than a CPU guarantee.
    pub fn estimated_work(sample_rate: u32, bandwidth_hz: u32) -> u64 {
        let mut rate = sample_rate as u64;
        let mut work = rate * 8; // NCO and translation allowance.
        while rate / 2 >= (u64::from(bandwidth_hz) * 4).max(48_000) {
            rate /= 2;
            work += rate * 31;
        }
        work + rate * channel_taps(rate as f64, bandwidth_hz) as u64
    }
    /// State persists across arbitrary input blocks. Output capacity is reused.
    pub fn process(&mut self, input: &[Complex32], output: &mut Vec<Complex32>) {
        output.clear();
        for &sample in input {
            let mut value =
                Some(sample * Complex32::new(self.oscillator.re as f32, self.oscillator.im as f32));
            self.oscillator *= self.step;
            self.phase_count += 1;
            if self.phase_count == 4096 {
                self.oscillator /= self.oscillator.norm();
                self.phase_count = 0;
            }
            for stage in &mut self.stages {
                value = value.and_then(|sample| stage.push(sample));
                if value.is_none() {
                    break;
                }
            }
            if let Some(value) = value.and_then(|sample| self.filter.push(sample)) {
                output.push(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tone(rate: u32, frequency: f64, count: usize) -> Vec<Complex32> {
        (0..count)
            .map(|n| Complex32::from_polar(0.5, (TAU * frequency * n as f64 / rate as f64) as f32))
            .collect()
    }
    #[test]
    fn translation_filtering_and_decimation_reject_adjacent_signal() {
        let rate = 2_000_000;
        let mut channel = Channelizer::new(rate, 300_000.0, 12_000).unwrap();
        let mut wanted = Vec::new();
        channel.process(&tone(rate, 301_000.0, 131072), &mut wanted);
        assert_eq!(channel.output_rate(), 62_500.0);
        assert_eq!(wanted.len(), 4096);
        let power = |v: &[Complex32]| {
            v[1024..].iter().map(|x| x.norm_sqr()).sum::<f32>() / (v.len() - 1024) as f32
        };
        let expected_power = power(&wanted);
        assert!((expected_power - 0.25).abs() < 0.02);
        let phase: f32 = wanted[1024..]
            .windows(2)
            .map(|s| (s[1] * s[0].conj()).arg())
            .sum();
        let frequency = phase as f64 / (wanted.len() - 1025) as f64 * channel.output_rate() / TAU;
        assert!((frequency - 1000.0).abs() < 2.0);
        let mut unwanted = Vec::new();
        Channelizer::new(rate, 300_000.0, 12_000)
            .unwrap()
            .process(&tone(rate, 325_000.0, 131072), &mut unwanted);
        assert!(power(&unwanted) / expected_power < 0.0001);
    }
    #[test]
    fn arbitrary_block_boundaries_preserve_phase_and_filter_state() {
        let input = tone(200_000, -31_000.0, 10000);
        let mut whole = Vec::new();
        Channelizer::new(200_000, -30_000.0, 10_000)
            .unwrap()
            .process(&input, &mut whole);
        let mut split = Channelizer::new(200_000, -30_000.0, 10_000).unwrap();
        let mut joined = Vec::new();
        let mut chunk = Vec::new();
        for input in input.chunks(127) {
            split.process(input, &mut chunk);
            joined.extend_from_slice(&chunk);
        }
        assert_eq!(whole, joined);
        assert!(Channelizer::new(2_000_000, 999_000.0, 12_000).is_err());
    }
}
