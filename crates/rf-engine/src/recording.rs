//! Integrity-prioritized bounded SigMF writer for raw signed interleaved IQ.
use rf_device::stream::{RawIqSink, BLOCK_BYTES};
use rf_types::DeviceState;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const RECORD_POOL_BLOCKS: usize = 32;
const CHANNEL_BLOCKS: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SigmfMeta {
    pub global: SigmfGlobal,
    pub captures: Vec<SigmfCapture>,
    pub annotations: Vec<serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SigmfGlobal {
    #[serde(rename = "core:version")]
    pub core_version: String,
    #[serde(rename = "core:datatype")]
    pub datatype: String,
    #[serde(rename = "core:sample_rate")]
    pub sample_rate: u32,
    #[serde(rename = "core:frequency")]
    pub frequency: u64,
    #[serde(rename = "core:author")]
    pub author: String,
    #[serde(rename = "core:description")]
    pub description: String,
    #[serde(rename = "core:hw")]
    pub hw: String,
    #[serde(rename = "rfscope:recorder")]
    pub recorder: String,
    #[serde(rename = "rfscope:session_id")]
    pub session_id: String,
    #[serde(rename = "rfscope:start_time_unix_ns")]
    pub start_time_unix_ns: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SigmfCapture {
    #[serde(rename = "core:sample_start")]
    pub core_sample_start: u64,
    #[serde(rename = "core:frequency")]
    pub frequency: u64,
    #[serde(rename = "core:sample_rate")]
    pub sample_rate: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordingSummary {
    pub id: String,
    pub directory: String,
    pub data_path: String,
    pub meta_path: String,
    pub active: bool,
    pub sample_rate_hz: u32,
    pub center_frequency_hz: u64,
    pub elapsed_ms: u64,
    pub bytes_written: u64,
    pub samples_written: u64,
    pub queued_blocks: usize,
    pub dropped_blocks: u64,
    pub dropped_bytes: u64,
    pub write_errors: u64,
    pub last_error: Option<String>,
    pub projected_bytes_per_second: u64,
    pub available_disk_bytes: Option<u64>,
    pub metadata_write_errors: u64,
}

struct Block {
    bytes: Vec<u8>,
    len: usize,
}
struct Shared {
    free: Mutex<Vec<Block>>,
    queued: AtomicU64,
    dropped_blocks: AtomicU64,
    dropped_bytes: AtomicU64,
    write_errors: AtomicU64,
    metadata_write_errors: AtomicU64,
    samples_written: AtomicU64,
    last_error: Mutex<Option<String>>,
    active: AtomicBool,
}
pub struct RecordingSink {
    tx: Mutex<Option<mpsc::SyncSender<Block>>>,
    shared: Arc<Shared>,
}
impl RecordingSink {
    fn push(&self, bytes: &[u8]) {
        if !self.shared.active.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut free) = self.shared.free.try_lock() else {
            self.drop(bytes.len());
            return;
        };
        let Some(mut block) = free.pop() else {
            self.drop(bytes.len());
            return;
        };
        if bytes.len() > block.bytes.len() {
            self.drop(bytes.len());
            free.push(block);
            return;
        }
        block.bytes[..bytes.len()].copy_from_slice(bytes);
        block.len = bytes.len();
        drop(free);
        let Ok(tx) = self.tx.lock() else {
            self.drop(bytes.len());
            return;
        };
        let Some(tx) = tx.as_ref() else {
            self.drop(bytes.len());
            return;
        };
        match tx.try_send(block) {
            Ok(()) => {
                self.shared.queued.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::TrySendError::Full(block)) => {
                self.shared.dropped_blocks.fetch_add(1, Ordering::Relaxed);
                self.shared
                    .dropped_bytes
                    .fetch_add(block.len as u64, Ordering::Relaxed);
                if let Ok(mut free) = self.shared.free.lock() {
                    free.push(block);
                }
            }
            Err(mpsc::TrySendError::Disconnected(block)) => {
                self.shared.write_errors.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut free) = self.shared.free.lock() {
                    free.push(block);
                }
            }
        }
    }
    fn drop(&self, len: usize) {
        self.shared.dropped_blocks.fetch_add(1, Ordering::Relaxed);
        self.shared
            .dropped_bytes
            .fetch_add(len as u64, Ordering::Relaxed);
    }
}
impl RawIqSink for RecordingSink {
    fn push(&self, bytes: &[u8]) {
        self.push(bytes);
    }
}
pub struct Recording {
    sink: Arc<RecordingSink>,
    summary: Arc<Mutex<RecordingSummary>>,
    metadata: Mutex<SigmfMeta>,
    meta_path: PathBuf,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Recording {
    pub fn start(
        root: impl AsRef<Path>,
        center_frequency_hz: u64,
        sample_rate_hz: u32,
        hardware: impl Into<String>,
    ) -> io::Result<Self> {
        if sample_rate_hz == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sample rate must be nonzero",
            ));
        }
        let root = root.as_ref();
        fs::create_dir_all(root)?;
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |v| v.as_nanos());
        let id = format!("capture-{epoch:x}");
        let directory = root.join(&id);
        fs::create_dir(&directory)?;
        let data_path = directory.join(format!("{id}.sigmf-data"));
        let meta_path = directory.join(format!("{id}.sigmf-meta"));
        let data = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&data_path)?;
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |v| v.as_nanos() as u64);
        let meta = SigmfMeta {
            global: SigmfGlobal {
                core_version: "1.0.0".into(),
                datatype: "ci8_le".into(),
                sample_rate: sample_rate_hz,
                frequency: center_frequency_hz,
                author: "RFScope".into(),
                description: "RFScope receive IQ capture".into(),
                hw: hardware.into(),
                recorder: "RFScope".into(),
                session_id: id.clone(),
                start_time_unix_ns: started,
            },
            captures: vec![SigmfCapture {
                core_sample_start: 0,
                frequency: center_frequency_hz,
                sample_rate: sample_rate_hz,
            }],
            annotations: vec![],
        };
        let meta_text = serde_json::to_vec_pretty(&meta).map_err(io::Error::other)?;
        let mut meta_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&meta_path)?;
        meta_file.write_all(&meta_text)?;
        meta_file.write_all(b"\n")?;
        meta_file.sync_all()?;
        let shared = Arc::new(Shared {
            free: Mutex::new(
                (0..RECORD_POOL_BLOCKS)
                    .map(|_| Block {
                        bytes: vec![0; BLOCK_BYTES],
                        len: 0,
                    })
                    .collect(),
            ),
            queued: AtomicU64::new(0),
            dropped_blocks: AtomicU64::new(0),
            dropped_bytes: AtomicU64::new(0),
            write_errors: AtomicU64::new(0),
            metadata_write_errors: AtomicU64::new(0),
            samples_written: AtomicU64::new(0),
            last_error: Mutex::new(None),
            active: AtomicBool::new(true),
        });
        let (tx, rx) = mpsc::sync_channel(CHANNEL_BLOCKS);
        let writer_shared = shared.clone();
        let summary = Arc::new(Mutex::new(RecordingSummary {
            id: id.clone(),
            directory: directory.display().to_string(),
            data_path: data_path.display().to_string(),
            meta_path: meta_path.display().to_string(),
            active: true,
            sample_rate_hz,
            center_frequency_hz,
            elapsed_ms: 0,
            bytes_written: 0,
            samples_written: 0,
            queued_blocks: 0,
            dropped_blocks: 0,
            dropped_bytes: 0,
            write_errors: 0,
            last_error: None,
            projected_bytes_per_second: u64::from(sample_rate_hz) * 2,
            available_disk_bytes: fs2::available_space(&directory).ok(),
            metadata_write_errors: 0,
        }));
        let writer_summary = summary.clone();
        let join = std::thread::Builder::new()
            .name(format!("sigmf-writer-{id}"))
            .spawn(move || writer_loop(data, rx, writer_shared, writer_summary))
            .map_err(io::Error::other)?;
        Ok(Self {
            sink: Arc::new(RecordingSink {
                tx: Mutex::new(Some(tx)),
                shared,
            }),
            summary,
            metadata: Mutex::new(meta),
            meta_path,
            join: Some(join),
        })
    }
    pub fn sink(&self) -> Arc<RecordingSink> {
        self.sink.clone()
    }
    pub fn summary(&self) -> RecordingSummary {
        let mut summary =
            self.summary
                .lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| RecordingSummary {
                    id: "unknown".into(),
                    directory: String::new(),
                    data_path: String::new(),
                    meta_path: String::new(),
                    active: false,
                    sample_rate_hz: 0,
                    center_frequency_hz: 0,
                    elapsed_ms: 0,
                    bytes_written: 0,
                    samples_written: 0,
                    queued_blocks: 0,
                    dropped_blocks: 0,
                    dropped_bytes: 0,
                    write_errors: 1,
                    last_error: Some("summary lock poisoned".into()),
                    projected_bytes_per_second: 0,
                    available_disk_bytes: None,
                    metadata_write_errors: 1,
                });
        if !summary.directory.is_empty() {
            summary.available_disk_bytes = fs2::available_space(&summary.directory).ok();
        }
        summary.metadata_write_errors = self
            .sink
            .shared
            .metadata_write_errors
            .load(Ordering::Relaxed);
        summary
    }
    /// Updates SigMF metadata after a control-plane change, never from the IQ callback.
    pub fn record_device_state(
        &self,
        state: &DeviceState,
        requested_patch: serde_json::Value,
    ) -> io::Result<()> {
        let sample_start = self.sink.shared.samples_written.load(Ordering::Acquire);
        let mut metadata = self
            .metadata
            .lock()
            .map_err(|_| io::Error::other("recording metadata lock poisoned"))?;
        let changed_capture = metadata.captures.last().is_none_or(|capture| {
            capture.frequency != state.center_frequency_hz
                || capture.sample_rate != state.sample_rate_hz
        });
        if changed_capture {
            metadata.captures.push(SigmfCapture {
                core_sample_start: sample_start,
                frequency: state.center_frequency_hz,
                sample_rate: state.sample_rate_hz,
            });
        }
        metadata.annotations.push(serde_json::json!({
            "core:sample_start": sample_start,
            "rfscope:event": "device_settings_changed",
            "rfscope:device_state": state,
            "rfscope:requested_patch": requested_patch,
        }));
        let temporary = self.meta_path.with_extension("sigmf-meta.tmp");
        let encoded = serde_json::to_vec_pretty(&*metadata).map_err(io::Error::other)?;
        let result = (|| -> io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temporary)?;
            file.write_all(&encoded)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, &self.meta_path)
        })();
        if result.is_err() {
            self.sink
                .shared
                .metadata_write_errors
                .fetch_add(1, Ordering::Relaxed);
        }
        result
    }
    pub fn finish(mut self) -> io::Result<RecordingSummary> {
        self.sink.shared.active.store(false, Ordering::Release);
        self.sink
            .tx
            .lock()
            .map_err(|_| io::Error::other("recording sender lock poisoned"))?
            .take();
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| io::Error::other("recording writer panicked"))?;
        }
        let summary = self.summary();
        if summary.dropped_blocks > 0
            || summary.write_errors > 0
            || summary.metadata_write_errors > 0
        {
            return Err(io::Error::other(format!(
                "recording incomplete: {} dropped blocks, {} data write errors, {} metadata write errors",
                summary.dropped_blocks, summary.write_errors, summary.metadata_write_errors
            )));
        }
        Ok(summary)
    }
}
fn writer_loop(
    mut data: File,
    rx: mpsc::Receiver<Block>,
    shared: Arc<Shared>,
    summary: Arc<Mutex<RecordingSummary>>,
) {
    let started = std::time::Instant::now();
    let mut bytes = 0u64;
    while let Ok(block) = rx.recv() {
        shared.queued.fetch_sub(1, Ordering::Relaxed);
        if let Err(error) = data
            .write_all(&block.bytes[..block.len])
            .and_then(|_| data.flush())
        {
            shared.write_errors.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut last) = shared.last_error.lock() {
                *last = Some(error.to_string());
            }
        } else {
            bytes += block.len as u64;
        }
        if let Ok(mut free) = shared.free.lock() {
            free.push(block);
        }
        if let Ok(mut value) = summary.lock() {
            value.bytes_written = bytes;
            value.samples_written = bytes / 2;
            shared.samples_written.store(bytes / 2, Ordering::Release);
            value.elapsed_ms = started.elapsed().as_millis() as u64;
            value.queued_blocks = shared.queued.load(Ordering::Relaxed) as usize;
            value.dropped_blocks = shared.dropped_blocks.load(Ordering::Relaxed);
            value.dropped_bytes = shared.dropped_bytes.load(Ordering::Relaxed);
            value.write_errors = shared.write_errors.load(Ordering::Relaxed);
            value.metadata_write_errors = shared.metadata_write_errors.load(Ordering::Relaxed);
            value.last_error = shared.last_error.lock().ok().and_then(|v| v.clone());
        }
    }
    if let Ok(mut value) = summary.lock() {
        value.active = false;
        value.queued_blocks = 0;
        value.elapsed_ms = started.elapsed().as_millis() as u64;
        value.dropped_blocks = shared.dropped_blocks.load(Ordering::Relaxed);
        value.dropped_bytes = shared.dropped_bytes.load(Ordering::Relaxed);
        value.write_errors = shared.write_errors.load(Ordering::Relaxed);
        value.metadata_write_errors = shared.metadata_write_errors.load(Ordering::Relaxed);
    }
}

pub fn validate_sigmf(
    meta_path: impl AsRef<Path>,
    data_path: impl AsRef<Path>,
) -> io::Result<SigmfMeta> {
    let text = fs::read_to_string(meta_path)?;
    let meta: SigmfMeta = serde_json::from_str(&text).map_err(io::Error::other)?;
    if meta.global.datatype != "ci8_le"
        || meta.global.sample_rate == 0
        || meta.global.frequency == 0
        || meta.captures.is_empty()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported or incomplete SigMF metadata",
        ));
    }
    let len = fs::metadata(data_path)?.len();
    if len % 2 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ci8_le data has odd byte length",
        ));
    }
    Ok(meta)
}

pub struct RecordingManager {
    root: PathBuf,
    current: Mutex<Option<Recording>>,
}
impl RecordingManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            current: Mutex::new(None),
        }
    }
    pub fn start(&self, center: u64, rate: u32, hardware: String) -> io::Result<RecordingSummary> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| io::Error::other("recording manager lock poisoned"))?;
        if current.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a recording is already active",
            ));
        }
        let recording = Recording::start(&self.root, center, rate, hardware)?;
        let summary = recording.summary();
        *current = Some(recording);
        Ok(summary)
    }
    pub fn stop(&self) -> io::Result<RecordingSummary> {
        let recording = self
            .current
            .lock()
            .map_err(|_| io::Error::other("recording manager lock poisoned"))?
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no active recording"))?;
        recording.finish()
    }
    pub fn status(&self) -> Option<RecordingSummary> {
        self.current
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|r| r.summary()))
    }
    pub fn sink(&self) -> Option<Arc<dyn RawIqSink>> {
        self.current
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|r| r.sink() as Arc<dyn RawIqSink>))
    }
    pub fn record_device_state(
        &self,
        state: &DeviceState,
        requested_patch: serde_json::Value,
    ) -> io::Result<()> {
        let current = self
            .current
            .lock()
            .map_err(|_| io::Error::other("recording manager lock poisoned"))?;
        if let Some(recording) = current.as_ref() {
            recording.record_device_state(state, requested_patch)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn creates_unique_sigmf_and_reports_bounded_overload() {
        let root = std::env::temp_dir().join(format!("rfscope-recording-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let recording = Recording::start(&root, 100_000_000, 2_000_000, "HackRF Pro r1.2").unwrap();
        let sink = recording.sink();
        let block: Vec<u8> = (0..BLOCK_BYTES)
            .map(|i| if i % 2 == 0 { 1 } else { 2 })
            .collect();
        for _ in 0..1000 {
            sink.push(&block);
        }
        let summary = recording.finish();
        assert!(summary.is_err());
        let dirs = fs::read_dir(&root).unwrap().count();
        assert_eq!(dirs, 1);
        let directory = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
        let id = directory.file_name().unwrap().to_string_lossy();
        let meta = validate_sigmf(
            directory.join(format!("{id}.sigmf-meta")),
            directory.join(format!("{id}.sigmf-data")),
        )
        .unwrap();
        assert_eq!(meta.global.datatype, "ci8_le");
        assert_eq!(meta.global.sample_rate, 2_000_000);
        let _ = fs::remove_dir_all(&root);
    }
    #[test]
    fn clean_recording_writes_exact_samples_and_no_overwrite() {
        let root =
            std::env::temp_dir().join(format!("rfscope-recording-clean-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let recording = Recording::start(&root, 100_000_000, 48_000, "mock").unwrap();
        let sink = recording.sink();
        let block: Vec<u8> = (0..2048).map(|i| if i % 2 == 0 { 1 } else { 2 }).collect();
        for _ in 0..4 {
            sink.push(&block);
        }
        recording
            .record_device_state(
                &DeviceState {
                    center_frequency_hz: 100_100_000,
                    sample_rate_hz: 96_000,
                    running: true,
                },
                serde_json::json!({ "center_frequency_hz": 100_100_000 }),
            )
            .unwrap();
        let summary = recording.finish().unwrap();
        assert_eq!(summary.bytes_written, 8192);
        assert_eq!(summary.samples_written, 4096);
        let directory = PathBuf::from(summary.directory);
        let id = directory.file_name().unwrap().to_string_lossy();
        let meta = validate_sigmf(
            directory.join(format!("{id}.sigmf-meta")),
            directory.join(format!("{id}.sigmf-data")),
        )
        .unwrap();
        assert_eq!(meta.captures[0].frequency, 100_000_000);
        assert_eq!(meta.captures[1].frequency, 100_100_000);
        assert_eq!(meta.captures[1].sample_rate, 96_000);
        assert_eq!(
            meta.annotations[0]["rfscope:event"],
            "device_settings_changed"
        );
        let second = Recording::start(&root, 100_000_000, 48_000, "mock").unwrap();
        assert_ne!(second.summary().id, summary.id);
        drop(second);
        let _ = fs::remove_dir_all(&root);
    }
}
