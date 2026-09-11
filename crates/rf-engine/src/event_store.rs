//! Bounded asynchronous persistence for completed spectrum events.
use crate::storage::Storage;
use rf_types::SignalEvent;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const EVENT_QUEUE_CAPACITY: usize = 128;

pub struct EventStore {
    sender: SyncSender<SignalEvent>,
    shutdown: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    dropped: Arc<AtomicU64>,
}

impl EventStore {
    pub fn start(storage: Arc<Storage>) -> Self {
        let (sender, receiver) = sync_channel(EVENT_QUEUE_CAPACITY);
        let shutdown = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let worker_shutdown = shutdown.clone();
        let worker = thread::Builder::new()
            .name("event-store".into())
            .spawn(move || persist_loop(storage, receiver, worker_shutdown))
            .ok();
        Self {
            sender,
            shutdown,
            worker: Mutex::new(worker),
            dropped,
        }
    }

    /// This runs on the DSP loop and never waits for SQLite.
    pub fn enqueue(&self, event: SignalEvent) {
        match self.sender.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

impl Drop for EventStore {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn persist_loop(storage: Arc<Storage>, receiver: Receiver<SignalEvent>, shutdown: Arc<AtomicBool>) {
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                if let Err(error) = storage.record_signal_event(&event) {
                    tracing::error!(%error, event_id = %event.id, "failed to persist signal event");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if shutdown.load(Ordering::Acquire) => {
                for event in receiver.try_iter() {
                    if let Err(error) = storage.record_signal_event(&event) {
                        tracing::error!(%error, event_id = %event.id, "failed to persist signal event");
                    }
                }
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_events_are_written_off_the_producer_path() {
        let storage = Arc::new(Storage::in_memory().unwrap());
        let writer = EventStore::start(storage.clone());
        writer.enqueue(SignalEvent {
            id: "event-test".into(),
            start_frequency_hz: 100,
            end_frequency_hz: 101,
            start_time_unix_ns: 1,
            end_time_unix_ns: Some(2),
            peak_dbfs: -10.0,
            snr_db: 20.0,
        });
        writer.shutdown();
        assert_eq!(storage.signal_events(1).unwrap()[0].id, "event-test");
        assert_eq!(writer.dropped(), 0);
    }
}
