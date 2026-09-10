//! Shared live/file-ready IQ pipeline and spectrum wire encoder.
use bytes::{BufMut, Bytes, BytesMut};
use rf_device::{IqSource, MockSource};
use rf_dsp::{SpectrumAnalyzer, Window};
use rf_types::*;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, RwLock};
pub const SPECTRUM_HEADER_BYTES: usize = 48;
pub struct Engine {
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
        Diagnostics {
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
    pub async fn run(self: Arc<Self>) {
        let mut source = MockSource::default();
        let mut configured = (0, 0);
        let mut sequence = 0u64;
        loop {
            let state = self.state.read().await.clone();
            let size = *self.fft_size.read().await;
            if !state.running {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
            if configured != (state.center_frequency_hz, state.sample_rate_hz) {
                if source
                    .configure(state.center_frequency_hz, state.sample_rate_hz)
                    .is_err()
                {
                    continue;
                }
                configured = (state.center_frequency_hz, state.sample_rate_hz);
            }
            let mut iq = vec![Default::default(); size];
            if source.read(&mut iq).await.is_err() {
                continue;
            }
            self.received.fetch_add(size as u64, Ordering::Relaxed);
            let Ok(mut fft) = SpectrumAnalyzer::new(size, Window::BlackmanHarris) else {
                continue;
            };
            let mut bins = Vec::with_capacity(size);
            if fft.process(&iq, &mut bins).is_err() {
                continue;
            }
            sequence += 1;
            let bytes = encode_spectrum(
                sequence,
                state.center_frequency_hz,
                state.sample_rate_hz,
                &bins,
            );
            if self.frames.send(bytes).is_err() {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            self.frame_count.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
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
    #[test]
    fn frame_has_documented_size() {
        let f = encode_spectrum(3, 100, 2, &[-1.0, -2.0]);
        assert_eq!(f.len(), 56);
        assert_eq!(&f[..4], b"RFSP");
        assert_eq!(u16::from_le_bytes([f[4], f[5]]), 1);
    }
}
