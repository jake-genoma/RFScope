//! Capability-driven SDR source abstraction and deterministic mock backend.
pub mod control;
pub mod file;
#[cfg(feature = "hackrf")]
mod hackrf;
pub mod stream;
use async_trait::async_trait;
use num_complex::Complex32;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rf_types::{DeviceCapabilities, DeviceDescriptor, DeviceState, NumericRange};
use std::f64::consts::TAU;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("no IQ available")]
    NoData,
    #[error("IQ stream: {0}")]
    Stream(String),
    #[error("file source: {0}")]
    File(String),
    #[error("playback reached end of file")]
    EndOfFile,
    #[error("value {value} is outside {minimum}..={maximum}")]
    OutOfRange {
        value: u64,
        minimum: u64,
        maximum: u64,
    },
}

#[async_trait]
pub trait IqSource: Send {
    fn descriptor(&self) -> DeviceDescriptor;
    fn capabilities(&self) -> DeviceCapabilities;
    fn state(&self) -> DeviceState;
    fn configure(&mut self, center_hz: u64, sample_rate_hz: u32) -> Result<(), DeviceError>;
    async fn read(&mut self, output: &mut [Complex32]) -> Result<usize, DeviceError>;
}

pub struct MockSource {
    state: DeviceState,
    sample_index: u64,
    rng: ChaCha8Rng,
}

impl Default for MockSource {
    fn default() -> Self {
        Self::new(100_000_000, 2_000_000)
    }
}
impl MockSource {
    pub fn new(center_frequency_hz: u64, sample_rate_hz: u32) -> Self {
        Self {
            state: DeviceState {
                center_frequency_hz,
                sample_rate_hz,
                running: true,
            },
            sample_index: 0,
            rng: ChaCha8Rng::seed_from_u64(0x52465343),
        }
    }
}

#[async_trait]
impl IqSource for MockSource {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor {
            id: "mock-0".into(),
            name: "Deterministic RF scene".into(),
            driver: "mock".into(),
        }
    }
    fn capabilities(&self) -> DeviceCapabilities {
        DeviceCapabilities {
            frequency_hz: NumericRange {
                min: 1_000_000,
                max: 6_000_000_000,
                step: 1,
            },
            sample_rate_hz: NumericRange {
                min: 200_000,
                max: 20_000_000,
                step: 1,
            },
            gain_stages: vec![],
            baseband_filter_bandwidths_hz: vec![],
            supports_sweep: false,
            max_sweep_ranges: None,
        }
    }
    fn state(&self) -> DeviceState {
        self.state.clone()
    }
    fn configure(&mut self, center_hz: u64, rate: u32) -> Result<(), DeviceError> {
        let caps = self.capabilities();
        if center_hz < caps.frequency_hz.min || center_hz > caps.frequency_hz.max {
            return Err(DeviceError::OutOfRange {
                value: center_hz,
                minimum: caps.frequency_hz.min,
                maximum: caps.frequency_hz.max,
            });
        }
        if u64::from(rate) < caps.sample_rate_hz.min || u64::from(rate) > caps.sample_rate_hz.max {
            return Err(DeviceError::OutOfRange {
                value: u64::from(rate),
                minimum: caps.sample_rate_hz.min,
                maximum: caps.sample_rate_hz.max,
            });
        }
        self.state.center_frequency_hz = center_hz;
        self.state.sample_rate_hz = rate;
        Ok(())
    }
    async fn read(&mut self, output: &mut [Complex32]) -> Result<usize, DeviceError> {
        let rate = self.state.sample_rate_hz as f64;
        for (offset, sample) in output.iter_mut().enumerate() {
            let t = (self.sample_index + offset as u64) as f64 / rate;
            let cw = (TAU * (-300_000.0) * t).sin_cos();
            let am = (1.0 + 0.55 * (TAU * 1_000.0 * t).sin()) * 0.35;
            let carrier = (TAU * 0.0 * t).sin_cos();
            let fm_phase = TAU * 400_000.0 * t + 2.2 * (TAU * 1_500.0 * t).sin();
            let fm = fm_phase.sin_cos();
            *sample = Complex32::new(
                (cw.1 * 0.22 + carrier.1 * am + fm.1 * 0.25) as f32
                    + self.rng.random_range(-0.025..0.025),
                (cw.0 * 0.22 + carrier.0 * am + fm.0 * 0.25) as f32
                    + self.rng.random_range(-0.025..0.025),
            );
        }
        self.sample_index += output.len() as u64;
        Ok(output.len())
    }
}
