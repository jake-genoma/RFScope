//! Low-rate receiver registry and DSP-thread-owned channel state.
use rf_dsp::channel::Channelizer;
use rf_types::{DeviceState, Vfo, VfoConfiguration};
use std::{
    collections::BTreeMap,
    sync::RwLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_WORK_BUDGET: u64 = 1_000_000_000;
pub struct VfoBank {
    receivers: RwLock<BTreeMap<String, Vfo>>,
    next: std::sync::atomic::AtomicU64,
    session: u128,
    pub work_budget: u64,
}
impl Default for VfoBank {
    fn default() -> Self {
        Self::new(DEFAULT_WORK_BUDGET)
    }
}
impl VfoBank {
    pub fn new(work_budget: u64) -> Self {
        Self {
            receivers: RwLock::new(BTreeMap::new()),
            next: std::sync::atomic::AtomicU64::new(1),
            session: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |t| t.as_nanos()),
            work_budget,
        }
    }
    pub fn list(&self) -> Result<Vec<Vfo>, String> {
        Ok(self
            .receivers
            .read()
            .map_err(|_| "receiver registry poisoned")?
            .values()
            .cloned()
            .collect())
    }
    pub fn put(
        &self,
        id: Option<&str>,
        config: VfoConfiguration,
        capture: &DeviceState,
    ) -> Result<Vfo, String> {
        validate(&config, capture)?;
        let mut receivers = self
            .receivers
            .write()
            .map_err(|_| "receiver registry poisoned")?;
        if id.is_some_and(|id| !receivers.contains_key(id)) {
            return Err("receiver does not exist".into());
        }
        let work = receivers
            .values()
            .filter(|v| Some(v.id.as_str()) != id)
            .fold(0u64, |sum, v| {
                sum.saturating_add(Channelizer::estimated_work(
                    capture.sample_rate_hz,
                    v.configuration.bandwidth_hz,
                ))
            });
        if work.saturating_add(Channelizer::estimated_work(
            capture.sample_rate_hz,
            config.bandwidth_hz,
        )) > self.work_budget
        {
            return Err(
                "receiver processing budget exceeded for this sample rate/bandwidth".into(),
            );
        }
        let id = id.map(str::to_owned).unwrap_or_else(|| {
            format!(
                "vfo-{:x}-{}",
                self.session,
                self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )
        });
        let receiver = Vfo {
            id: id.clone(),
            configuration: config,
            recording: false,
            output_rate_hz: 0.0,
            processed_samples: receivers.get(&id).map_or(0, |v| v.processed_samples),
            channel_power_dbfs: -120.0,
            suspended_reason: None,
        };
        receivers.insert(id, receiver.clone());
        Ok(receiver)
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        self.receivers
            .write()
            .map_err(|_| "receiver registry poisoned")?
            .remove(id)
            .ok_or("receiver does not exist")?;
        Ok(())
    }
    fn publish(&self, receiver: Vfo) {
        if let Ok(mut receivers) = self.receivers.write() {
            if let Some(current) = receivers.get_mut(&receiver.id) {
                if current.configuration == receiver.configuration {
                    *current = receiver;
                }
            }
        }
    }
}
pub fn validate(config: &VfoConfiguration, capture: &DeviceState) -> Result<(), String> {
    if config.name.trim().is_empty() || config.name.len() > 128 {
        return Err("receiver name must contain 1–128 bytes".into());
    }
    if !(500..=100_000).contains(&config.bandwidth_hz) {
        return Err("channel bandwidth must be 500–100000 Hz".into());
    }
    if !config.volume.is_finite() || !(0.0..=2.0).contains(&config.volume) {
        return Err("volume must be 0–2".into());
    }
    if config
        .squelch_dbfs
        .is_some_and(|v| !v.is_finite() || !(-120.0..=0.0).contains(&v))
    {
        return Err("squelch must be null or -120–0 dBFS".into());
    }
    let offset = (i128::from(config.frequency_hz) - i128::from(capture.center_frequency_hz)).abs();
    if capture.sample_rate_hz == 0
        || 2 * offset + i128::from(config.bandwidth_hz) > i128::from(capture.sample_rate_hz)
    {
        return Err("receiver passband lies outside instantaneous capture bandwidth".into());
    }
    Ok(())
}

struct Runtime {
    config: VfoConfiguration,
    center: u64,
    rate: u32,
    channel: Channelizer,
    output: Vec<num_complex::Complex32>,
}
#[derive(Default)]
pub struct VfoProcessor {
    channels: BTreeMap<String, Runtime>,
}
impl VfoProcessor {
    pub fn reset(&mut self) {
        self.channels.clear();
    }
    pub fn process(
        &mut self,
        bank: &VfoBank,
        capture: &DeviceState,
        iq: &[num_complex::Complex32],
    ) {
        let Ok(receivers) = bank.list() else {
            return;
        };
        self.channels
            .retain(|id, _| receivers.iter().any(|v| &v.id == id));
        let mut budget = bank.work_budget;
        for mut receiver in receivers {
            let config = &receiver.configuration;
            let cost = Channelizer::estimated_work(capture.sample_rate_hz, config.bandwidth_hz);
            let valid = validate(config, capture).and_then(|()| {
                if cost > budget {
                    Err("capture rate exceeds receiver processing budget".into())
                } else {
                    budget -= cost;
                    Ok(())
                }
            });
            if let Err(reason) = valid {
                self.channels.remove(&receiver.id);
                receiver.suspended_reason = Some(reason);
                receiver.output_rate_hz = 0.0;
                bank.publish(receiver);
                continue;
            }
            let replace = self.channels.get(&receiver.id).is_none_or(|r| {
                r.center != capture.center_frequency_hz
                    || r.rate != capture.sample_rate_hz
                    || r.config.frequency_hz != config.frequency_hz
                    || r.config.bandwidth_hz != config.bandwidth_hz
                    || r.config.mode != config.mode
            });
            if replace {
                let offset = (i128::from(config.frequency_hz)
                    - i128::from(capture.center_frequency_hz)) as f64;
                let Ok(channel) =
                    Channelizer::new(capture.sample_rate_hz, offset, config.bandwidth_hz)
                else {
                    continue;
                };
                self.channels.insert(
                    receiver.id.clone(),
                    Runtime {
                        config: config.clone(),
                        center: capture.center_frequency_hz,
                        rate: capture.sample_rate_hz,
                        channel,
                        output: Vec::new(),
                    },
                );
            }
            if let Some(runtime) = self.channels.get_mut(&receiver.id) {
                runtime.channel.process(iq, &mut runtime.output);
                receiver.output_rate_hz = runtime.channel.output_rate();
                receiver.processed_samples += runtime.output.len() as u64;
                if !runtime.output.is_empty() {
                    let power = runtime.output.iter().map(|x| x.norm_sqr()).sum::<f32>()
                        / runtime.output.len() as f32;
                    receiver.channel_power_dbfs = 10.0 * power.max(1e-12).log10();
                }
                receiver.suspended_reason = None;
                bank.publish(receiver);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rf_types::ReceiverMode;
    fn config(frequency_hz: u64) -> VfoConfiguration {
        VfoConfiguration {
            name: "Receiver".into(),
            frequency_hz,
            mode: ReceiverMode::Am,
            bandwidth_hz: 10000,
            squelch_dbfs: None,
            agc: true,
            volume: 1.0,
            mute: false,
            solo: false,
        }
    }
    #[test]
    fn stable_ids_capture_validation_and_resource_budget() {
        let capture = DeviceState {
            center_frequency_hz: 100_000_000,
            sample_rate_hz: 200_000,
            running: true,
        };
        let bank = VfoBank::default();
        let receiver = bank
            .put(None, config(capture.center_frequency_hz), &capture)
            .unwrap();
        let edited = bank
            .put(
                Some(&receiver.id),
                config(capture.center_frequency_hz + 1000),
                &capture,
            )
            .unwrap();
        assert_eq!(receiver.id, edited.id);
        assert!(bank
            .put(
                None,
                config(capture.center_frequency_hz + 100_000),
                &capture
            )
            .is_err());
        assert!(bank.put(None, config(u64::MAX), &capture).is_err());
        for _ in 0..11 {
            bank.put(None, config(capture.center_frequency_hz), &capture)
                .unwrap();
        }
        assert_eq!(bank.list().unwrap().len(), 12); // No arbitrary ten-receiver limit.
        bank.remove(&receiver.id).unwrap();
        assert!(bank.remove(&receiver.id).is_err());
        let tiny = VfoBank::new(1);
        assert!(tiny
            .put(None, config(capture.center_frequency_hz), &capture)
            .is_err());
    }
    #[test]
    fn hardware_capture_change_suspends_out_of_band_receivers() {
        let mut capture = DeviceState {
            center_frequency_hz: 100_000_000,
            sample_rate_hz: 200_000,
            running: true,
        };
        let bank = VfoBank::default();
        bank.put(None, config(capture.center_frequency_hz), &capture)
            .unwrap();
        let mut dsp = VfoProcessor::default();
        let input = vec![num_complex::Complex32::new(0.5, 0.0); 1000];
        dsp.process(&bank, &capture, &input);
        assert!(bank.list().unwrap()[0].processed_samples > 0);
        capture.center_frequency_hz += 1_000_000;
        dsp.process(&bank, &capture, &input);
        assert!(bank.list().unwrap()[0].suspended_reason.is_some());
    }
}
