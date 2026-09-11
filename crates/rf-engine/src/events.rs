//! Bounded event lifecycle tracking over uncalibrated spectrum measurements.
use rf_dsp::analysis::SpectrumMeasurements;
use rf_types::SignalEvent;
use std::collections::VecDeque;

const MAX_EVENTS: usize = 4096;
const QUIET_FRAMES_TO_CLOSE: u8 = 3;

struct ActiveEvent {
    event: SignalEvent,
    quiet_frames: u8,
}

pub struct EventTracker {
    threshold_db: f32,
    next_id: u64,
    active: Option<ActiveEvent>,
    completed: VecDeque<SignalEvent>,
}

impl Default for EventTracker {
    fn default() -> Self {
        Self {
            threshold_db: 12.0,
            next_id: 1,
            active: None,
            completed: VecDeque::with_capacity(MAX_EVENTS),
        }
    }
}

impl EventTracker {
    /// Returns an event only when its quiet-frame lifecycle completes.
    pub fn observe(
        &mut self,
        timestamp_ns: u64,
        measurement: &SpectrumMeasurements,
    ) -> Option<SignalEvent> {
        if measurement.snr_db >= self.threshold_db {
            if let Some(active) = self.active.as_mut() {
                active.quiet_frames = 0;
                active.event.end_frequency_hz =
                    measurement.peak_frequency_hz.round().max(0.0) as u64;
                active.event.peak_dbfs = active.event.peak_dbfs.max(measurement.peak_dbfs);
                active.event.snr_db = active.event.snr_db.max(measurement.snr_db);
            } else {
                let frequency = measurement.peak_frequency_hz.round().max(0.0) as u64;
                self.active = Some(ActiveEvent {
                    event: SignalEvent {
                        id: format!("event-{timestamp_ns:x}-{:x}", self.next_id),
                        start_frequency_hz: frequency,
                        end_frequency_hz: frequency,
                        start_time_unix_ns: timestamp_ns,
                        end_time_unix_ns: None,
                        peak_dbfs: measurement.peak_dbfs,
                        snr_db: measurement.snr_db,
                    },
                    quiet_frames: 0,
                });
                self.next_id += 1;
            }
            return None;
        }
        let active = self.active.as_mut()?;
        active.quiet_frames = active.quiet_frames.saturating_add(1);
        if active.quiet_frames < QUIET_FRAMES_TO_CLOSE {
            return None;
        }
        let active = self.active.take()?;
        let mut completed = active.event;
        completed.end_time_unix_ns = Some(timestamp_ns);
        if self.completed.len() >= MAX_EVENTS {
            self.completed.pop_front();
        }
        self.completed.push_back(completed.clone());
        Some(completed)
    }
    pub fn events(&self) -> Vec<SignalEvent> {
        let mut events: Vec<_> = self.completed.iter().cloned().collect();
        if let Some(active) = &self.active {
            events.push(active.event.clone());
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn measurement(snr_db: f32) -> SpectrumMeasurements {
        SpectrumMeasurements {
            peak_frequency_hz: 100_000_100.0,
            peak_dbfs: -10.0,
            noise_floor_dbfs: -80.0,
            snr_db,
            bandwidth_3db_hz: 1.0,
            bandwidth_6db_hz: 2.0,
            occupied_bandwidth_99_hz: 3.0,
            amplitude_mean_dbfs: -70.0,
            amplitude_min_dbfs: -90.0,
            amplitude_max_dbfs: -10.0,
        }
    }
    #[test]
    fn event_has_bounded_lifecycle_and_duration() {
        let mut tracker = EventTracker::default();
        assert!(tracker.observe(10, &measurement(20.0)).is_none());
        assert_eq!(tracker.events().len(), 1);
        assert!(tracker.observe(20, &measurement(0.0)).is_none());
        assert!(tracker.observe(30, &measurement(0.0)).is_none());
        assert!(tracker.observe(40, &measurement(0.0)).is_some());
        let event = tracker.events().pop().unwrap();
        assert_eq!(event.start_time_unix_ns, 10);
        assert_eq!(event.end_time_unix_ns, Some(40));
        assert_eq!(event.peak_dbfs, -10.0);
    }
}
