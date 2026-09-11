//! SigMF IQ source used by playback; it implements the same `IqSource` boundary as live RX.
use crate::{DeviceError, IqSource};
use async_trait::async_trait;
use num_complex::Complex32;
use rf_types::{DeviceCapabilities, DeviceDescriptor, DeviceState, NumericRange};
use serde::Deserialize;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
struct Meta {
    global: Global,
    captures: Vec<Capture>,
}
#[derive(Deserialize)]
struct Global {
    #[serde(rename = "core:datatype")]
    datatype: String,
    #[serde(rename = "core:sample_rate")]
    sample_rate: u32,
    #[serde(rename = "core:frequency")]
    frequency: u64,
    #[serde(default, rename = "core:hw")]
    hw: String,
    #[serde(default, rename = "rfscope:session_id")]
    session_id: String,
}
#[allow(dead_code)]
#[derive(Deserialize)]
struct Capture {
    #[serde(rename = "core:sample_start")]
    sample_start: u64,
    #[serde(rename = "core:frequency")]
    frequency: Option<u64>,
    #[serde(rename = "core:sample_rate")]
    sample_rate: Option<u32>,
}
pub struct SigmfSource {
    file: File,
    data_path: PathBuf,
    descriptor: DeviceDescriptor,
    capabilities: DeviceCapabilities,
    state: DeviceState,
    total_samples: u64,
    position_samples: u64,
    paused: bool,
    scratch: Vec<u8>,
    pub metadata_path: PathBuf,
    pub hardware: String,
    pub session_id: String,
}
impl SigmfSource {
    pub fn open(meta_path: impl AsRef<Path>) -> Result<Self, DeviceError> {
        let metadata_path = meta_path.as_ref().to_path_buf();
        let text = std::fs::read_to_string(&metadata_path)
            .map_err(|e| DeviceError::File(e.to_string()))?;
        let meta: Meta = serde_json::from_str(&text)
            .map_err(|e| DeviceError::File(format!("invalid SigMF metadata: {e}")))?;
        if meta.global.datatype != "ci8_le"
            || meta.global.sample_rate == 0
            || meta.global.frequency == 0
            || meta.captures.is_empty()
        {
            return Err(DeviceError::File(
                "only ci8_le captures with nonzero frequency/rate are supported".into(),
            ));
        }
        let data_path = metadata_path.with_extension("sigmf-data");
        let file = File::open(&data_path).map_err(|e| DeviceError::File(e.to_string()))?;
        let bytes = file
            .metadata()
            .map_err(|e| DeviceError::File(e.to_string()))?
            .len();
        if bytes % 2 != 0 {
            return Err(DeviceError::File(
                "IQ data length is not a whole number of ci8_le samples".into(),
            ));
        }
        let id = if meta.global.session_id.is_empty() {
            metadata_path
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("recording")
                .to_owned()
        } else {
            meta.global.session_id
        };
        Ok(Self {
            file,
            data_path,
            descriptor: DeviceDescriptor {
                id: format!("file:{id}"),
                name: format!("SigMF {id}"),
                driver: "sigmf".into(),
            },
            capabilities: DeviceCapabilities {
                frequency_hz: NumericRange {
                    min: 1,
                    max: 6_000_000_000,
                    step: 1,
                },
                sample_rate_hz: NumericRange {
                    min: 1,
                    max: u64::from(meta.global.sample_rate),
                    step: 1,
                },
                gain_stages: vec![],
                baseband_filter_bandwidths_hz: vec![],
                supports_sweep: false,
                max_sweep_ranges: None,
            },
            state: DeviceState {
                center_frequency_hz: meta.global.frequency,
                sample_rate_hz: meta.global.sample_rate,
                running: false,
            },
            total_samples: bytes / 2,
            position_samples: 0,
            paused: true,
            scratch: Vec::new(),
            metadata_path,
            hardware: meta.global.hw,
            session_id: id,
        })
    }
    pub fn play(&mut self) {
        self.paused = false;
        self.state.running = true;
    }
    pub fn pause(&mut self) {
        self.paused = true;
        self.state.running = false;
    }
    pub fn seek_samples(&mut self, position: u64) -> Result<(), DeviceError> {
        self.position_samples = position.min(self.total_samples);
        self.file
            .seek(SeekFrom::Start(self.position_samples * 2))
            .map_err(|e| DeviceError::File(e.to_string()))?;
        Ok(())
    }
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }
    pub fn position_samples(&self) -> u64 {
        self.position_samples
    }
    pub fn data_path(&self) -> &Path {
        &self.data_path
    }
    pub fn read_block(&mut self, output: &mut [Complex32]) -> Result<usize, DeviceError> {
        if self.paused {
            return Err(DeviceError::NoData);
        }
        if self.position_samples >= self.total_samples {
            self.pause();
            return Err(DeviceError::EndOfFile);
        }
        let count = output
            .len()
            .min((self.total_samples - self.position_samples) as usize);
        self.scratch.resize(count * 2, 0);
        self.file
            .read_exact(&mut self.scratch)
            .map_err(|e| DeviceError::File(e.to_string()))?;
        for (sample, pair) in output
            .iter_mut()
            .zip(self.scratch.as_chunks::<2>().0.iter())
        {
            *sample = Complex32::new(pair[0] as i8 as f32 / 128.0, pair[1] as i8 as f32 / 128.0);
        }
        self.position_samples += count as u64;
        Ok(count)
    }
}
#[async_trait]
impl IqSource for SigmfSource {
    fn descriptor(&self) -> DeviceDescriptor {
        self.descriptor.clone()
    }
    fn capabilities(&self) -> DeviceCapabilities {
        self.capabilities.clone()
    }
    fn state(&self) -> DeviceState {
        self.state.clone()
    }
    fn configure(&mut self, center: u64, rate: u32) -> Result<(), DeviceError> {
        if center != self.state.center_frequency_hz || rate != self.state.sample_rate_hz {
            return Err(DeviceError::File(
                "recorded center frequency and rate cannot be changed".into(),
            ));
        }
        Ok(())
    }
    async fn read(&mut self, output: &mut [Complex32]) -> Result<usize, DeviceError> {
        self.read_block(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[tokio::test]
    async fn sigmf_source_reuses_iq_boundary_and_seek_is_bounded() {
        let root = std::env::temp_dir().join(format!("rfscope-playback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let meta = root.join("test.sigmf-meta");
        let data = root.join("test.sigmf-data");
        let mut f = File::create(&data).unwrap();
        f.write_all(&[128, 0, 64, 128, 0, 255, 127, 1]).unwrap();
        std::fs::write(&meta,r#"{"global":{"core:datatype":"ci8_le","core:sample_rate":48000,"core:frequency":100000000,"core:hw":"mock","rfscope:session_id":"test"},"captures":[{"core:sample_start":0}]}"#).unwrap();
        let mut source = SigmfSource::open(&meta).unwrap();
        assert_eq!(source.total_samples(), 4);
        source.play();
        let mut out = vec![Complex32::default(); 8];
        assert_eq!(source.read(&mut out).await.unwrap(), 4);
        assert_eq!(out[0], Complex32::new(-1.0, 0.0));
        source.seek_samples(2).unwrap();
        assert_eq!(source.read(&mut out).await.unwrap(), 2);
        assert!(matches!(
            source.read(&mut out).await,
            Err(DeviceError::EndOfFile)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
