//! Stable domain and wire-adjacent types shared by RFScope services.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct DeviceDescriptor {
    pub id: String,
    pub name: String,
    pub driver: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct NumericRange {
    pub min: u64,
    pub max: u64,
    pub step: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GainStage {
    pub id: String,
    pub label: String,
    pub range: NumericRange,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceCapabilities {
    pub frequency_hz: NumericRange,
    pub sample_rate_hz: NumericRange,
    pub gain_stages: Vec<GainStage>,
    pub supports_sweep: bool,
    pub max_sweep_ranges: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceState {
    pub center_frequency_hz: u64,
    pub sample_rate_hz: u32,
    pub running: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceStatePatch {
    pub center_frequency_hz: Option<u64>,
    pub sample_rate_hz: Option<u32>,
    pub running: Option<bool>,
    pub fft_size: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostics {
    pub received_samples: u64,
    pub fft_frames: u64,
    pub dropped_visualization_frames: u64,
    pub websocket_clients: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub name: &'static str,
    pub version: &'static str,
    pub source: &'static str,
    pub state: DeviceState,
    pub fft_size: usize,
    pub diagnostics: Diagnostics,
}
