//! Bounded application-owned signed-IQ transfer pool, independent of native handles.
use crate::{DeviceError, IqSource};
use async_trait::async_trait;
use num_complex::Complex32;
use rf_types::{DeviceCapabilities, DeviceDescriptor, DeviceState};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

pub const BLOCK_BYTES: usize = 262_144;
pub const POOL_BLOCKS: usize = 16;
struct Block {
    bytes: Vec<u8>,
    length: usize,
}
struct Pool {
    free: Vec<Block>,
    ready: VecDeque<Block>,
}
#[derive(Default)]
pub struct StreamCounters {
    pub bytes: AtomicU64,
    pub blocks: AtomicU64,
    pub dropped_blocks: AtomicU64,
    pub dropped_bytes: AtomicU64,
    pub invalid_blocks: AtomicU64,
    pub running: AtomicBool,
    pub stream_faults: AtomicU64,
}
/// Callback only tries the pool lock, copies bytes and increments atomics. No waiting,
/// allocation, DSP, logging, native calls or disk I/O occurs on this path.
pub struct Ingress {
    pool: Mutex<Pool>,
    pub counters: Arc<StreamCounters>,
}
impl Default for Ingress {
    fn default() -> Self {
        Self {
            pool: Mutex::new(Pool {
                free: (0..POOL_BLOCKS)
                    .map(|_| Block {
                        bytes: vec![0; BLOCK_BYTES],
                        length: 0,
                    })
                    .collect(),
                ready: VecDeque::with_capacity(POOL_BLOCKS),
            }),
            counters: Arc::new(StreamCounters::default()),
        }
    }
}
impl Ingress {
    pub fn push(&self, bytes: &[u8]) {
        self.counters.blocks.fetch_add(1, Ordering::Relaxed);
        self.counters
            .bytes
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        if bytes.is_empty() || !bytes.len().is_multiple_of(2) || bytes.len() > BLOCK_BYTES {
            self.counters.invalid_blocks.fetch_add(1, Ordering::Relaxed);
            self.drop_block(bytes.len());
            return;
        }
        if let Ok(mut pool) = self.pool.try_lock() {
            if let Some(mut block) = pool.free.pop() {
                block.bytes[..bytes.len()].copy_from_slice(bytes);
                block.length = bytes.len();
                pool.ready.push_back(block);
                return;
            }
        }
        self.drop_block(bytes.len());
    }
    fn drop_block(&self, bytes: usize) {
        self.counters.dropped_blocks.fetch_add(1, Ordering::Relaxed);
        self.counters
            .dropped_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }
}
/// Safe IQ adapter transferred to the DSP worker; no native pointer crosses threads.
pub struct BufferedSource {
    pub ingress: Arc<Ingress>,
    descriptor: DeviceDescriptor,
    capabilities: DeviceCapabilities,
    state: DeviceState,
}
impl BufferedSource {
    pub fn new(
        descriptor: DeviceDescriptor,
        capabilities: DeviceCapabilities,
        state: DeviceState,
    ) -> Self {
        Self {
            ingress: Arc::new(Ingress::default()),
            descriptor,
            capabilities,
            state,
        }
    }
}
#[async_trait]
impl IqSource for BufferedSource {
    fn descriptor(&self) -> DeviceDescriptor {
        self.descriptor.clone()
    }
    fn capabilities(&self) -> DeviceCapabilities {
        self.capabilities.clone()
    }
    fn state(&self) -> DeviceState {
        let mut state = self.state.clone();
        state.running = self.ingress.counters.running.load(Ordering::Acquire);
        state
    }
    fn configure(&mut self, _: u64, _: u32) -> Result<(), DeviceError> {
        Err(DeviceError::Stream(
            "configure hardware through its device owner".into(),
        ))
    }
    async fn read(&mut self, output: &mut [Complex32]) -> Result<usize, DeviceError> {
        if !self.state().running {
            return Err(DeviceError::NoData);
        }
        let block = self
            .ingress
            .pool
            .lock()
            .map_err(|_| DeviceError::Stream("IQ pool poisoned".into()))?
            .ready
            .pop_front();
        let Some(block) = block else {
            return Err(DeviceError::NoData);
        };
        let count = block.length / 2;
        let result = if output.len() < count {
            Err(DeviceError::Stream(
                "IQ output is smaller than transfer block".into(),
            ))
        } else {
            for (sample, pair) in output
                .iter_mut()
                .zip(block.bytes[..block.length].as_chunks::<2>().0)
            {
                *sample =
                    Complex32::new(pair[0] as i8 as f32 / 128.0, pair[1] as i8 as f32 / 128.0);
            }
            Ok(count)
        };
        self.ingress
            .pool
            .lock()
            .map_err(|_| DeviceError::Stream("IQ pool poisoned".into()))?
            .free
            .push(block);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn signed_conversion_recycles_buffers_and_respects_stop() {
        use crate::MockSource;
        let mock = MockSource::default();
        let mut source = BufferedSource::new(mock.descriptor(), mock.capabilities(), mock.state());
        source
            .ingress
            .counters
            .running
            .store(true, Ordering::Release);
        let mut output = vec![Complex32::default(); 4];
        for _ in 0..POOL_BLOCKS * 3 {
            source.ingress.push(&[128, 127, 0, 255]);
            assert_eq!(source.read(&mut output).await.unwrap(), 2);
            assert_eq!(output[0], Complex32::new(-1.0, 127.0 / 128.0));
            assert_eq!(output[1], Complex32::new(0.0, -1.0 / 128.0));
        }
        assert_eq!(source.ingress.pool.lock().unwrap().free.len(), POOL_BLOCKS);
        source.ingress.push(&[1, 2]);
        source
            .ingress
            .counters
            .running
            .store(false, Ordering::Release);
        assert!(matches!(
            source.read(&mut output).await,
            Err(DeviceError::NoData)
        ));
    }
    #[test]
    fn pool_overload_is_bounded_and_explicit() {
        let ingress = Ingress::default();
        for _ in 0..POOL_BLOCKS + 3 {
            ingress.push(&[128, 127]);
        }
        assert_eq!(ingress.pool.lock().unwrap().ready.len(), POOL_BLOCKS);
        assert_eq!(ingress.counters.dropped_blocks.load(Ordering::Relaxed), 3);
        assert_eq!(ingress.counters.dropped_bytes.load(Ordering::Relaxed), 6);
        ingress.push(&[1]);
        assert_eq!(ingress.counters.invalid_blocks.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn callback_contention_drops_without_waiting() {
        let ingress = Ingress::default();
        let _guard = ingress.pool.lock().unwrap();
        ingress.push(&[0, 0]);
        assert_eq!(ingress.counters.dropped_blocks.load(Ordering::Relaxed), 1);
    }
}
