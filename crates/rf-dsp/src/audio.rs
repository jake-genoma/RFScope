//! Fixed-rate mono audio with anti-alias resampling and per-receiver controls.
use std::f64::consts::{PI, TAU};
pub const AUDIO_RATE: u32 = 48_000;
const PHASES: usize = 64;

#[derive(Clone, Copy)]
pub struct AudioControls {
    pub volume: f32,
    pub agc: bool,
    pub audible: bool,
    pub squelch_dbfs: Option<f32>,
}
pub struct AudioPipeline {
    input_rate: f64,
    filters: Vec<Vec<f32>>,
    delay: Vec<f32>,
    cursor: usize,
    phase: f64,
    hp_feedback: f32,
    hp_input: f32,
    hp_output: f32,
    energy: f32,
    agc_gain: f32,
    gate: f32,
    hold: u32,
    pub squelch_open: bool,
}
impl AudioPipeline {
    pub fn tap_count(input_rate: f64, lowpass_hz: u32) -> usize {
        ((8.0 * input_rate / lowpass_hz.max(1) as f64).ceil() as usize | 1).clamp(63, 4095)
    }
    pub fn new(input_rate: f64, highpass_hz: u32, lowpass_hz: u32) -> Result<Self, &'static str> {
        if !input_rate.is_finite()
            || !(8000.0..=2_000_000.0).contains(&input_rate)
            || !(500..=12000).contains(&lowpass_hz)
            || highpass_hz > 1000
            || highpass_hz >= lowpass_hz
        {
            return Err("invalid audio rate or filter settings");
        }
        let length = Self::tap_count(input_rate, lowpass_hz);
        let middle = (length - 1) as f64 / 2.0;
        let cutoff = (lowpass_hz as f64)
            .min(input_rate * 0.45)
            .min(AUDIO_RATE as f64 * 0.45)
            / input_rate;
        let filters = (0..PHASES)
            .map(|phase| {
                let fraction = phase as f64 / PHASES as f64;
                let mut taps: Vec<f32> = (0..length)
                    .map(|n| {
                        let x = n as f64 - middle - fraction;
                        let sinc = if x.abs() < 1e-12 {
                            2.0 * cutoff
                        } else {
                            (TAU * cutoff * x).sin() / (PI * x)
                        };
                        let angle = TAU * n as f64 / (length - 1) as f64;
                        (sinc * (0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos())) as f32
                    })
                    .collect();
                let sum: f32 = taps.iter().sum();
                for tap in &mut taps {
                    *tap /= sum;
                }
                taps
            })
            .collect();
        Ok(Self {
            input_rate,
            filters,
            delay: vec![0.0; length],
            cursor: 0,
            phase: 0.0,
            hp_feedback: (-TAU * highpass_hz as f64 / AUDIO_RATE as f64).exp() as f32,
            hp_input: 0.0,
            hp_output: 0.0,
            energy: 0.01,
            agc_gain: 1.0,
            gate: 0.0,
            hold: 0,
            squelch_open: false,
        })
    }
    pub fn process(
        &mut self,
        input: &[f32],
        power_dbfs: f32,
        controls: AudioControls,
        output: &mut Vec<f32>,
    ) {
        output.clear();
        let above = controls.squelch_dbfs.is_none_or(|threshold| {
            power_dbfs >= threshold + if self.squelch_open { 0.0 } else { 3.0 }
        });
        if above {
            self.hold = AUDIO_RATE / 10;
        }
        for &sample in input {
            self.delay[self.cursor] = if sample.is_finite() { sample } else { 0.0 };
            self.phase += AUDIO_RATE as f64;
            while self.phase >= self.input_rate {
                self.phase -= self.input_rate;
                let fraction = self.phase / AUDIO_RATE as f64;
                let phase = ((fraction * PHASES as f64) as usize).min(PHASES - 1);
                let mut filtered = 0.0;
                let mut index = self.cursor;
                for &tap in &self.filters[phase] {
                    filtered += tap * self.delay[index];
                    index = if index == 0 {
                        self.delay.len() - 1
                    } else {
                        index - 1
                    };
                }
                let highpass = filtered - self.hp_input + self.hp_feedback * self.hp_output;
                self.hp_input = filtered;
                self.hp_output = highpass;
                self.energy += 0.002 * (highpass * highpass - self.energy);
                let target = if controls.agc {
                    (0.15 / self.energy.max(1e-6).sqrt()).clamp(0.05, 20.0)
                } else {
                    1.0
                };
                self.agc_gain +=
                    (if target < self.agc_gain { 0.02 } else { 0.0002 }) * (target - self.agc_gain);
                if !above {
                    self.hold = self.hold.saturating_sub(1);
                }
                self.squelch_open = above || self.hold > 0;
                let gate = if self.squelch_open && controls.audible {
                    1.0
                } else {
                    0.0
                };
                self.gate += (gate - self.gate).clamp(-1.0 / 240.0, 1.0 / 240.0);
                output.push(
                    (highpass * self.agc_gain * controls.volume * self.gate).clamp(-1.0, 1.0),
                );
            }
            self.cursor = (self.cursor + 1) % self.delay.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn controls() -> AudioControls {
        AudioControls {
            volume: 1.0,
            agc: false,
            audible: true,
            squelch_dbfs: None,
        }
    }
    fn tone(rate: usize, hz: f64, amplitude: f32) -> Vec<f32> {
        (0..rate)
            .map(|n| amplitude * (TAU * hz * n as f64 / rate as f64).sin() as f32)
            .collect()
    }
    fn rms(input: &[f32]) -> f32 {
        (input.iter().map(|x| x * x).sum::<f32>() / input.len() as f32).sqrt()
    }
    #[test]
    fn resampling_is_fixed_rate_continuous_and_rejects_aliases() {
        for rate in [48000, 62500, 78125, 100000, 400000] {
            let input = tone(rate, 1000.0, 0.5);
            let mut pipeline = AudioPipeline::new(rate as f64, 0, 5000).unwrap();
            let mut whole = Vec::new();
            pipeline.process(&input, -20.0, controls(), &mut whole);
            assert_eq!(whole.len(), 48000);
            assert!((rms(&whole[4800..]) - 0.5 / 2f32.sqrt()).abs() < 0.01);
            let mut split = AudioPipeline::new(rate as f64, 0, 5000).unwrap();
            let mut joined = Vec::new();
            let mut chunk = Vec::new();
            for input in input.chunks(257) {
                split.process(input, -20.0, controls(), &mut chunk);
                joined.extend_from_slice(&chunk);
            }
            assert_eq!(whole, joined);
            let mut rejected = Vec::new();
            AudioPipeline::new(rate as f64, 0, 5000).unwrap().process(
                &tone(rate, 19000.0, 0.5),
                -20.0,
                controls(),
                &mut rejected,
            );
            assert!(rms(&rejected[4800..]) < 0.002);
        }
    }
    #[test]
    fn volume_mute_squelch_and_agc_are_independent() {
        let input = tone(48000, 1000.0, 0.2);
        let mut output = Vec::new();
        let mut pipeline = AudioPipeline::new(48000.0, 0, 5000).unwrap();
        pipeline.process(
            &input,
            -20.0,
            AudioControls {
                volume: 0.5,
                ..controls()
            },
            &mut output,
        );
        assert!((rms(&output[4800..]) - 0.1 / 2f32.sqrt()).abs() < 0.005);
        pipeline.process(
            &input,
            -20.0,
            AudioControls {
                audible: false,
                ..controls()
            },
            &mut output,
        );
        assert!(output[4800..].iter().all(|&v| v == 0.0));
        pipeline.process(
            &input,
            -90.0,
            AudioControls {
                squelch_dbfs: Some(-60.0),
                ..controls()
            },
            &mut output,
        );
        assert!(!pipeline.squelch_open);
        assert!(output[9600..].iter().all(|&v| v == 0.0));
        pipeline.process(
            &input,
            -40.0,
            AudioControls {
                squelch_dbfs: Some(-60.0),
                agc: true,
                ..controls()
            },
            &mut output,
        );
        assert!(pipeline.squelch_open);
        assert!((rms(&output[24000..]) - 0.15).abs() < 0.02);
    }
}
