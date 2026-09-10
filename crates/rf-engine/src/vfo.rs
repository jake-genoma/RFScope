//! Low-rate receiver registry and DSP-thread-owned channel state.
use rf_dsp::{
    audio::{AudioControls, AudioPipeline, AUDIO_RATE},
    channel::Channelizer,
    demod::{Am, Demodulator, Nfm, Sideband, Ssb},
};
use rf_types::{DeviceState, ReceiverMode, Vfo, VfoConfiguration};
use std::{
    collections::BTreeMap,
    sync::RwLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_WORK_BUDGET: u64 = 1_000_000_000;
fn estimated_work(rate: u32, config: &VfoConfiguration) -> u64 {
    let mut output_rate = rate as f64;
    while output_rate / 2.0 >= (config.bandwidth_hz as f64 * 4.0).max(48_000.0) {
        output_rate /= 2.0;
    }
    let demod_work = match config.mode {
        ReceiverMode::Am => 8,
        ReceiverMode::Nfm => 32,
        ReceiverMode::Usb | ReceiverMode::Lsb => Ssb::tap_count(output_rate),
    };
    Channelizer::estimated_work(rate, config.bandwidth_hz)
        + output_rate as u64 * demod_work as u64
        + AUDIO_RATE as u64 * AudioPipeline::tap_count(output_rate, config.audio_lowpass_hz) as u64
}
pub struct VfoBank {
    pub audio: crate::audio::AudioBus,
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
            audio: crate::audio::AudioBus::default(),
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
                sum.saturating_add(estimated_work(capture.sample_rate_hz, &v.configuration))
            });
        if work.saturating_add(estimated_work(capture.sample_rate_hz, &config)) > self.work_budget {
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
            demodulated_samples: receivers.get(&id).map_or(0, |v| v.demodulated_samples),
            demodulated_peak: 0.0,
            audio_rate_hz: 0,
            audio_samples: receivers.get(&id).map_or(0, |v| v.audio_samples),
            audio_frames: receivers.get(&id).map_or(0, |v| v.audio_frames),
            audio_peak: 0.0,
            squelch_open: false,
            audio_active: false,
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
    if !(500..=12000).contains(&config.audio_lowpass_hz)
        || config.audio_highpass_hz > 1000
        || config.audio_highpass_hz >= config.audio_lowpass_hz
    {
        return Err("audio highpass must be 0–1000 Hz and below lowpass (500–12000 Hz)".into());
    }
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
    demodulator: Box<dyn Demodulator>,
    audio: Vec<f32>,
    audio_pipeline: AudioPipeline,
    pcm: Vec<f32>,
    packetizer: crate::audio::Packetizer,
}
#[derive(Default)]
pub struct VfoProcessor {
    channels: BTreeMap<String, Runtime>,
}
impl VfoProcessor {
    pub fn pause(&mut self, bank: &VfoBank) {
        // NoData while hardware is running only means a transfer has not arrived.
        // Explicit pauses are handled by the caller; clear state once, not per poll.
        if self.channels.is_empty() {
            return;
        }
        self.reset();
        if let Ok(receivers) = bank.list() {
            for mut receiver in receivers {
                receiver.audio_active = false;
                receiver.squelch_open = false;
                receiver.audio_peak = 0.0;
                bank.publish(receiver);
            }
        }
    }
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
        let any_solo = receivers.iter().any(|v| v.configuration.solo);
        for mut receiver in receivers {
            let config = &receiver.configuration;
            let cost = estimated_work(capture.sample_rate_hz, config);
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
                receiver.demodulated_peak = 0.0;
                receiver.audio_peak = 0.0;
                receiver.audio_active = false;
                receiver.squelch_open = false;
                bank.publish(receiver);
                continue;
            }
            let replace = self.channels.get(&receiver.id).is_none_or(|r| {
                r.center != capture.center_frequency_hz
                    || r.rate != capture.sample_rate_hz
                    || r.config.frequency_hz != config.frequency_hz
                    || r.config.bandwidth_hz != config.bandwidth_hz
                    || r.config.mode != config.mode
                    || r.config.audio_highpass_hz != config.audio_highpass_hz
                    || r.config.audio_lowpass_hz != config.audio_lowpass_hz
            });
            if replace {
                let offset = (i128::from(config.frequency_hz)
                    - i128::from(capture.center_frequency_hz)) as f64;
                let Ok(channel) =
                    Channelizer::new(capture.sample_rate_hz, offset, config.bandwidth_hz)
                else {
                    continue;
                };
                let rate = channel.output_rate();
                let demodulator: Result<Box<dyn Demodulator>, &'static str> = match config.mode {
                    ReceiverMode::Am => Am::new(rate).map(|v| Box::new(v) as Box<dyn Demodulator>),
                    ReceiverMode::Nfm => Nfm::new(
                        rate,
                        (config.bandwidth_hz as f32 / 5.0).clamp(500.0, 5000.0),
                    )
                    .map(|v| Box::new(v) as Box<dyn Demodulator>),
                    ReceiverMode::Usb => {
                        Ssb::new(rate, Sideband::Upper).map(|v| Box::new(v) as Box<dyn Demodulator>)
                    }
                    ReceiverMode::Lsb => {
                        Ssb::new(rate, Sideband::Lower).map(|v| Box::new(v) as Box<dyn Demodulator>)
                    }
                };
                let demodulator = match demodulator {
                    Ok(demodulator) => demodulator,
                    Err(error) => {
                        receiver.suspended_reason = Some(error.into());
                        bank.publish(receiver);
                        continue;
                    }
                };
                let audio_pipeline = match AudioPipeline::new(
                    rate,
                    config.audio_highpass_hz,
                    config.audio_lowpass_hz,
                ) {
                    Ok(pipeline) => pipeline,
                    Err(error) => {
                        receiver.suspended_reason = Some(error.into());
                        bank.publish(receiver);
                        continue;
                    }
                };
                self.channels.insert(
                    receiver.id.clone(),
                    Runtime {
                        config: config.clone(),
                        center: capture.center_frequency_hz,
                        rate: capture.sample_rate_hz,
                        channel,
                        output: Vec::new(),
                        demodulator,
                        audio: Vec::new(),
                        audio_pipeline,
                        pcm: Vec::new(),
                        packetizer: bank.audio.packetizer(),
                    },
                );
            }
            if let Some(runtime) = self.channels.get_mut(&receiver.id) {
                runtime.channel.process(iq, &mut runtime.output);
                runtime
                    .demodulator
                    .process(&runtime.output, &mut runtime.audio);
                receiver.demodulated_samples += runtime.audio.len() as u64;
                receiver.demodulated_peak =
                    runtime.audio.iter().map(|v| v.abs()).fold(0.0, f32::max);
                receiver.output_rate_hz = runtime.channel.output_rate();
                receiver.processed_samples += runtime.output.len() as u64;
                if !runtime.output.is_empty() {
                    let power = runtime.output.iter().map(|x| x.norm_sqr()).sum::<f32>()
                        / runtime.output.len() as f32;
                    receiver.channel_power_dbfs = 10.0 * power.max(1e-12).log10();
                }
                receiver.suspended_reason = None;
                let audible = !config.mute && (!any_solo || config.solo);
                runtime.audio_pipeline.process(
                    &runtime.audio,
                    receiver.channel_power_dbfs,
                    AudioControls {
                        volume: config.volume,
                        agc: config.agc,
                        audible,
                        squelch_dbfs: config.squelch_dbfs,
                    },
                    &mut runtime.pcm,
                );
                receiver.audio_rate_hz = AUDIO_RATE;
                receiver.audio_samples += runtime.pcm.len() as u64;
                receiver.audio_peak = runtime.pcm.iter().map(|v| v.abs()).fold(0.0, f32::max);
                receiver.squelch_open = runtime.audio_pipeline.squelch_open;
                receiver.audio_active = audible && receiver.squelch_open;
                let flags =
                    u16::from(receiver.squelch_open) | (u16::from(receiver.audio_active) << 1);
                receiver.audio_frames +=
                    runtime
                        .packetizer
                        .push(&receiver.id, &runtime.pcm, flags, &bank.audio);
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
            audio_highpass_hz: 80,
            audio_lowpass_hz: 5000,
        }
    }
    #[test]
    fn wideband_am_and_nfm_reach_normalized_demodulator_output() {
        let capture = DeviceState {
            center_frequency_hz: 100_000_000,
            sample_rate_hz: 200_000,
            running: true,
        };
        for mode in [ReceiverMode::Am, ReceiverMode::Nfm] {
            let bank = VfoBank::default();
            let mut configuration = config(capture.center_frequency_hz + 30_000);
            configuration.mode = mode;
            configuration.bandwidth_hz = 12_500;
            let receiver = bank.put(None, configuration, &capture).unwrap();
            let mut fm_phase = 0.0;
            let input: Vec<_> = (0..50000)
                .map(|n| {
                    let t = n as f64 / capture.sample_rate_hz as f64;
                    let tone = (std::f64::consts::TAU * 1000.0 * t).sin();
                    fm_phase +=
                        std::f64::consts::TAU * 2500.0 * 0.5 * tone / capture.sample_rate_hz as f64;
                    let phase = std::f64::consts::TAU * 30_000.0 * t
                        + if mode == ReceiverMode::Nfm {
                            fm_phase
                        } else {
                            0.0
                        };
                    let magnitude = if mode == ReceiverMode::Am {
                        0.4 * (1.0 + 0.5 * tone)
                    } else {
                        0.4
                    };
                    num_complex::Complex32::new(
                        (magnitude * phase.cos()) as f32,
                        (magnitude * phase.sin()) as f32,
                    )
                })
                .collect();
            let mut processor = VfoProcessor::default();
            processor.process(&bank, &capture, &input);
            let runtime = processor.channels.get(&receiver.id).unwrap();
            let samples = &runtime.audio[2500..];
            let mut i = 0.0;
            let mut q = 0.0;
            for (n, &sample) in samples.iter().enumerate() {
                let phase =
                    std::f64::consts::TAU * 1000.0 * n as f64 / runtime.channel.output_rate();
                i += sample as f64 * phase.cos();
                q += sample as f64 * phase.sin();
            }
            let recovered = 2.0 * i.hypot(q) / samples.len() as f64;
            assert!((recovered - 0.5).abs() < 0.03, "{mode:?}: {recovered}");
            let snapshot = &bank.list().unwrap()[0];
            assert_eq!(snapshot.demodulated_samples, snapshot.processed_samples);
            assert_eq!(snapshot.demodulated_samples, 12500);
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
