//! Shared live/file-ready IQ pipeline and spectrum wire encoder.
pub mod audio;
pub mod vfo;
use bytes::{BufMut, Bytes, BytesMut};
use rf_device::{
    control::DeviceController,
    stream::{StreamCounters, BLOCK_BYTES},
    DeviceError, IqSource, MockSource,
};
use rf_dsp::{SpectrumAnalyzer, Window};
use rf_types::*;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, RwLock};
pub const SPECTRUM_HEADER_BYTES: usize = 48;
pub struct Engine {
    pub vfos: vfo::VfoBank,
    pub hardware: AtomicBool,
    pub shutdown: AtomicBool,
    stream_counters: std::sync::RwLock<Option<Arc<StreamCounters>>>,
    pub state: Arc<RwLock<DeviceState>>,
    pub fft_size: Arc<RwLock<usize>>,
    pub frames: broadcast::Sender<Bytes>,
    received: Arc<AtomicU64>,
    frame_count: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    clients: Arc<AtomicU64>,
}
impl Engine {
    pub fn mock() -> Arc<Self> {
        let (frames, _) = broadcast::channel(4);
        Arc::new(Self {
            vfos: vfo::VfoBank::default(),
            hardware: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            stream_counters: std::sync::RwLock::new(None),
            state: Arc::new(RwLock::new(MockSource::default().state())),
            fft_size: Arc::new(RwLock::new(2048)),
            frames,
            received: Arc::new(AtomicU64::new(0)),
            frame_count: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            clients: Arc::new(AtomicU64::new(0)),
        })
    }
    pub fn diagnostics(&self) -> Diagnostics {
        let counters = self.stream_counters.read().ok();
        let counters = counters.as_deref().and_then(|c| c.as_ref());
        Diagnostics {
            audio_clients: self.vfos.audio.clients.load(Ordering::Relaxed),
            audio_lagged_frames: self.vfos.audio.lagged.load(Ordering::Relaxed),
            stream_faults: counters.map_or(0, |c| c.stream_faults.load(Ordering::Relaxed)),
            received_bytes: counters.map_or(0, |c| c.bytes.load(Ordering::Relaxed)),
            received_blocks: counters.map_or(0, |c| c.blocks.load(Ordering::Relaxed)),
            dropped_iq_blocks: counters.map_or(0, |c| c.dropped_blocks.load(Ordering::Relaxed)),
            dropped_iq_bytes: counters.map_or(0, |c| c.dropped_bytes.load(Ordering::Relaxed)),
            invalid_iq_blocks: counters.map_or(0, |c| c.invalid_blocks.load(Ordering::Relaxed)),
            hardware_streaming: counters.is_some_and(|c| c.running.load(Ordering::Acquire)),
            received_samples: self.received.load(Ordering::Relaxed),
            fft_frames: self.frame_count.load(Ordering::Relaxed),
            dropped_visualization_frames: self.dropped.load(Ordering::Relaxed),
            websocket_clients: self.clients.load(Ordering::Relaxed),
        }
    }
    pub fn client_connected(&self) {
        self.clients.fetch_add(1, Ordering::Relaxed);
    }
    pub fn client_disconnected(&self) {
        self.clients.fetch_sub(1, Ordering::Relaxed);
    }
    pub async fn run(self: Arc<Self>, devices: DeviceController) {
        let mut mock = MockSource::default();
        let mut hardware_source = None;
        let mut configured = (0, 0);
        let mut sequence = 0u64;
        let mut fft: Option<SpectrumAnalyzer> = None;
        let mut iq = vec![Default::default(); BLOCK_BYTES / 2];
        let mut bins = Vec::new();
        let mut vfos = vfo::VfoProcessor::default();
        let mut iq_drops = 0;
        let mut mock_deadline = tokio::time::Instant::now();
        let mut last_frame = std::time::Instant::now();
        while !self.shutdown.load(Ordering::Acquire) {
            if let Ok(mut pending) = devices.stream.lock() {
                if let Some(source) = pending.take() {
                    vfos.reset();
                    if let Ok(mut counters) = self.stream_counters.write() {
                        *counters = Some(source.ingress.counters.clone());
                    }
                    hardware_source = Some(source);
                }
            }
            let hardware = self.hardware.load(Ordering::Acquire);
            if !hardware && hardware_source.take().is_some() {
                if let Ok(mut counters) = self.stream_counters.write() {
                    *counters = None;
                }
            }
            let mut state = self.state.read().await.clone();
            let size = *self.fft_size.read().await;
            let result = if hardware {
                if let Some(source) = hardware_source.as_mut() {
                    state = source.state();
                    source.read(&mut iq).await
                } else {
                    Err(DeviceError::NoData)
                }
            } else if state.running {
                if configured != (state.center_frequency_hz, state.sample_rate_hz) {
                    if mock
                        .configure(state.center_frequency_hz, state.sample_rate_hz)
                        .is_err()
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                    configured = (state.center_frequency_hz, state.sample_rate_hz);
                }
                let count = (state.sample_rate_hz as usize / 100).clamp(size, iq.len());
                mock.read(&mut iq[..count]).await
            } else {
                Err(DeviceError::NoData)
            };
            let count = match result {
                Ok(count) => count,
                Err(DeviceError::NoData) => {
                    mock_deadline = tokio::time::Instant::now();
                    if !state.running {
                        vfos.pause(&self.vfos);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    continue;
                }
                Err(error) => {
                    tracing::error!(%error, "IQ source failed");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            };
            self.received.fetch_add(count as u64, Ordering::Relaxed);
            let drops = self.diagnostics().dropped_iq_blocks;
            if drops != iq_drops {
                vfos.reset();
                iq_drops = drops;
            }
            vfos.process(&self.vfos, &state, &iq[..count]);
            if !hardware {
                mock_deadline +=
                    std::time::Duration::from_secs_f64(count as f64 / state.sample_rate_hz as f64);
                tokio::time::sleep_until(mock_deadline).await;
            }
            if count < size || last_frame.elapsed() < std::time::Duration::from_millis(40) {
                continue;
            }
            if fft.as_ref().is_none_or(|f| f.size() != size) {
                fft = SpectrumAnalyzer::new(size, Window::BlackmanHarris).ok();
            }
            let Some(fft) = fft.as_mut() else {
                continue;
            };
            if fft.process(&iq[count - size..count], &mut bins).is_err() {
                continue;
            }
            sequence += 1;
            if self
                .frames
                .send(encode_spectrum(
                    sequence,
                    state.center_frequency_hz,
                    state.sample_rate_hz,
                    &bins,
                ))
                .is_err()
            {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            self.frame_count.fetch_add(1, Ordering::Relaxed);
            last_frame = std::time::Instant::now();
        }
    }
}
pub fn encode_spectrum(sequence: u64, center: u64, rate: u32, bins: &[f32]) -> Bytes {
    let mut b = BytesMut::with_capacity(SPECTRUM_HEADER_BYTES + bins.len() * 4);
    b.put_slice(b"RFSP");
    b.put_u16_le(1);
    b.put_u16_le(1);
    b.put_u32_le(SPECTRUM_HEADER_BYTES as u32);
    b.put_u64_le(sequence);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |v| v.as_nanos() as u64);
    b.put_u64_le(ts);
    b.put_u64_le(center);
    b.put_u32_le(rate);
    b.put_u32_le(bins.len() as u32);
    b.put_u32_le(0);
    for value in bins {
        b.put_f32_le(*value)
    }
    b.freeze()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn buffered_iq_uses_common_spectrum_and_stops_cleanly() {
        use rf_device::stream::BufferedSource;
        let mock = MockSource::default();
        let source = BufferedSource::new(mock.descriptor(), mock.capabilities(), mock.state());
        source
            .ingress
            .counters
            .running
            .store(true, Ordering::Release);
        let mut bytes = vec![0u8; 4096];
        for (index, pair) in bytes.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let phase = std::f32::consts::TAU * index as f32 / 8.0;
            pair[0] = (phase.cos() * 64.0) as i8 as u8;
            pair[1] = (phase.sin() * 64.0) as i8 as u8;
        }
        let ingress = source.ingress.clone();
        let controller = DeviceController::new().unwrap();
        *controller.stream.lock().unwrap() = Some(source);
        let engine = Engine::mock();
        engine.hardware.store(true, Ordering::Release);
        let mut frames = engine.frames.subscribe();
        let task = tokio::spawn(engine.clone().run(controller.clone()));
        tokio::time::sleep(std::time::Duration::from_millis(45)).await;
        ingress.push(&bytes);
        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), frames.recv())
            .await
            .unwrap()
            .unwrap();
        let bins: Vec<_> = frame[48..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let peak = bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        assert_eq!(peak.0, 1280);
        assert!((*peak.1 + 6.02).abs() < 0.2);
        assert_eq!(engine.diagnostics().received_samples, 2048);
        engine.shutdown.store(true, Ordering::Release);
        task.await.unwrap();
        controller.shutdown().unwrap();
    }
    #[test]
    fn frame_has_documented_size() {
        let f = encode_spectrum(3, 100, 2, &[-1.0, -2.0]);
        assert_eq!(f.len(), 56);
        assert_eq!(&f[..4], b"RFSP");
        assert_eq!(u16::from_le_bytes([f[4], f[5]]), 1);
    }
}
