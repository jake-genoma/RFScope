//! File playback control feeding the common IQ engine.
use rf_device::{file::SigmfSource, DeviceError, IqSource};
use serde::Serialize;
use std::{path::Path, sync::Mutex};

#[derive(Clone, Debug, Serialize)]
pub struct PlaybackSummary {
    pub loaded: bool,
    pub metadata_path: Option<String>,
    pub data_path: Option<String>,
    pub session_id: Option<String>,
    pub hardware: Option<String>,
    pub center_frequency_hz: u64,
    pub sample_rate_hz: u32,
    pub total_samples: u64,
    pub position_samples: u64,
    pub playing: bool,
    pub ended: bool,
}
pub struct PlaybackManager {
    source: Mutex<Option<SigmfSource>>,
}
impl Default for PlaybackManager {
    fn default() -> Self {
        Self {
            source: Mutex::new(None),
        }
    }
}
impl PlaybackManager {
    pub fn load(&self, path: impl AsRef<Path>) -> Result<PlaybackSummary, DeviceError> {
        let source = SigmfSource::open(path)?;
        let mut slot = self
            .source
            .lock()
            .map_err(|_| DeviceError::File("playback lock poisoned".into()))?;
        *slot = Some(source);
        Ok(self.summary_locked(&slot))
    }
    pub fn play(&self) -> Result<PlaybackSummary, DeviceError> {
        let mut slot = self
            .source
            .lock()
            .map_err(|_| DeviceError::File("playback lock poisoned".into()))?;
        slot.as_mut()
            .ok_or_else(|| DeviceError::File("no recording loaded".into()))?
            .play();
        Ok(self.summary_locked(&slot))
    }
    pub fn pause(&self) -> Result<PlaybackSummary, DeviceError> {
        let mut slot = self
            .source
            .lock()
            .map_err(|_| DeviceError::File("playback lock poisoned".into()))?;
        slot.as_mut()
            .ok_or_else(|| DeviceError::File("no recording loaded".into()))?
            .pause();
        Ok(self.summary_locked(&slot))
    }
    pub fn seek(&self, pos: u64) -> Result<PlaybackSummary, DeviceError> {
        let mut slot = self
            .source
            .lock()
            .map_err(|_| DeviceError::File("playback lock poisoned".into()))?;
        slot.as_mut()
            .ok_or_else(|| DeviceError::File("no recording loaded".into()))?
            .seek_samples(pos)?;
        Ok(self.summary_locked(&slot))
    }
    pub fn clear(&self) -> Result<(), DeviceError> {
        *self
            .source
            .lock()
            .map_err(|_| DeviceError::File("playback lock poisoned".into()))? = None;
        Ok(())
    }
    pub fn is_playing(&self) -> bool {
        self.source
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|s| s.state().running))
            .unwrap_or(false)
    }
    pub fn read(&self, out: &mut [num_complex::Complex32]) -> Result<usize, DeviceError> {
        let mut slot = self
            .source
            .lock()
            .map_err(|_| DeviceError::File("playback lock poisoned".into()))?;
        slot.as_mut().ok_or(DeviceError::NoData)?.read_block(out)
    }
    pub fn state(&self) -> Option<rf_types::DeviceState> {
        self.source
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|s| s.state()))
    }
    pub fn summary(&self) -> Option<PlaybackSummary> {
        self.source
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|_| self.summary_locked(&v)))
    }
    fn summary_locked(&self, slot: &Option<SigmfSource>) -> PlaybackSummary {
        let Some(source) = slot.as_ref() else {
            return PlaybackSummary {
                loaded: false,
                metadata_path: None,
                data_path: None,
                session_id: None,
                hardware: None,
                center_frequency_hz: 0,
                sample_rate_hz: 0,
                total_samples: 0,
                position_samples: 0,
                playing: false,
                ended: false,
            };
        };
        PlaybackSummary {
            loaded: true,
            metadata_path: Some(source.metadata_path.display().to_string()),
            data_path: Some(source.data_path().display().to_string()),
            session_id: Some(source.session_id.clone()),
            hardware: Some(source.hardware.clone()),
            center_frequency_hz: source.state().center_frequency_hz,
            sample_rate_hz: source.state().sample_rate_hz,
            total_samples: source.total_samples(),
            position_samples: source.position_samples(),
            playing: source.state().running,
            ended: source.position_samples() >= source.total_samples(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};
    #[test]
    fn playback_controls_file_source() {
        let root = std::env::temp_dir().join(format!("rfscope-pm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let meta = root.join("a.sigmf-meta");
        let mut data = fs::File::create(root.join("a.sigmf-data")).unwrap();
        data.write_all(&[1, 2, 3, 4]).unwrap();
        fs::write(&meta,r#"{"global":{"core:datatype":"ci8_le","core:sample_rate":48000,"core:frequency":100000000},"captures":[{"core:sample_start":0}]}"#).unwrap();
        let manager = PlaybackManager::default();
        assert!(manager.load(&meta).unwrap().loaded);
        manager.play().unwrap();
        assert!(manager.is_playing());
        assert_eq!(manager.seek(1).unwrap().position_samples, 1);
        manager.pause().unwrap();
        assert!(!manager.is_playing());
        manager.clear().unwrap();
        assert!(manager.summary().is_none());
        let _ = fs::remove_dir_all(root);
    }
}
