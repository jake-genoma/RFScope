//! Replaceable bounded audio transport boundary. The wire codec currently uses PCM.
use bytes::{BufMut, Bytes, BytesMut};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;
pub const AUDIO_FRAME_SAMPLES: usize = 960;
pub const AUDIO_HEADER_BYTES: usize = 48;
pub struct AudioBus {
    pub frames: broadcast::Sender<Bytes>,
    pub clients: AtomicU64,
    pub lagged: AtomicU64,
    epochs: AtomicU64,
}
impl Default for AudioBus {
    fn default() -> Self {
        Self {
            frames: broadcast::channel(32).0,
            clients: AtomicU64::new(0),
            lagged: AtomicU64::new(0),
            epochs: AtomicU64::new(1),
        }
    }
}
impl AudioBus {
    pub fn packetizer(&self) -> Packetizer {
        Packetizer {
            samples: [0.0; AUDIO_FRAME_SAMPLES],
            used: 0,
            sequence: 0,
            epoch: self.epochs.fetch_add(1, Ordering::Relaxed),
        }
    }
}
pub struct Packetizer {
    samples: [f32; AUDIO_FRAME_SAMPLES],
    used: usize,
    sequence: u64,
    epoch: u64,
}
impl Packetizer {
    pub fn push(&mut self, id: &str, input: &[f32], flags: u16, bus: &AudioBus) -> u64 {
        let before = self.sequence;
        for &sample in input {
            self.samples[self.used] = sample;
            self.used += 1;
            if self.used == AUDIO_FRAME_SAMPLES {
                self.sequence += 1;
                if bus.frames.receiver_count() > 0 {
                    let _ = bus.frames.send(encode_audio(
                        id,
                        self.epoch,
                        self.sequence,
                        flags,
                        &self.samples,
                    ));
                }
                self.used = 0;
            }
        }
        self.sequence - before
    }
}
fn encode_audio(
    id: &str,
    epoch: u64,
    sequence: u64,
    flags: u16,
    samples: &[f32; AUDIO_FRAME_SAMPLES],
) -> Bytes {
    let mut output = BytesMut::with_capacity(AUDIO_HEADER_BYTES + id.len() + samples.len() * 4);
    output.put_slice(b"RFAU");
    output.put_u16_le(1);
    output.put_u16_le(2);
    output.put_u32_le(AUDIO_HEADER_BYTES as u32);
    output.put_u64_le(sequence);
    output.put_u64_le(epoch);
    output.put_u32_le(rf_dsp::audio::AUDIO_RATE);
    output.put_u32_le(samples.len() as u32);
    output.put_u16_le(id.len() as u16);
    output.put_u16_le(flags);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_nanos() as u64);
    output.put_u64_le(timestamp);
    output.put_slice(id.as_bytes());
    for &sample in samples {
        output.put_f32_le(sample);
    }
    output.freeze()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_frames_are_bounded_and_epochs_change_on_reset() {
        let bus = AudioBus::default();
        let mut receiver = bus.frames.subscribe();
        let mut packetizer = bus.packetizer();
        assert_eq!(packetizer.push("vfo-1", &[0.25; 480], 3, &bus), 0);
        assert_eq!(packetizer.push("vfo-1", &[0.25; 480], 3, &bus), 1);
        let frame = receiver.try_recv().unwrap();
        assert_eq!(&frame[..4], b"RFAU");
        assert_eq!(frame.len(), 48 + 5 + 960 * 4);
        assert_eq!(u32::from_le_bytes(frame[28..32].try_into().unwrap()), 48000);
        assert_eq!(f32::from_le_bytes(frame[53..57].try_into().unwrap()), 0.25);
        assert_ne!(packetizer.epoch, bus.packetizer().epoch);
        for _ in 0..40 {
            packetizer.push("vfo-1", &[0.0; 960], 0, &bus);
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
    }
}
