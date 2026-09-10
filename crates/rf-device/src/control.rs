//! Low-rate device ownership/control, separate from the unchanged IQ source seam.
use crate::{IqSource, MockSource};
use rf_types::*;
use std::sync::mpsc::{self, SyncSender};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("invalid setting: {0}")]
    Invalid(String),
    #[error("device unavailable: {0}")]
    Unavailable(String),
    #[error("device ownership conflict: {0}")]
    Conflict(String),
    #[error("{operation}: {message} ({code}); USB errors may indicate permissions, another owner, or disconnect")]
    Native {
        operation: &'static str,
        code: i32,
        message: String,
    },
}
pub type Result<T> = std::result::Result<T, ControlError>;

pub(crate) trait ReceiverControl {
    fn snapshot(&self) -> DeviceSelection;
    fn configure(&mut self, config: ReceiverConfiguration) -> Result<()>;
    fn close(&mut self) -> Result<()>;
}

pub fn validate_configuration(
    caps: &DeviceCapabilities,
    config: &ReceiverConfiguration,
) -> Result<()> {
    fn check(name: &str, value: u64, range: &NumericRange) -> Result<()> {
        if value < range.min
            || value > range.max
            || range.step == 0
            || !(value - range.min).is_multiple_of(range.step)
        {
            return Err(ControlError::Invalid(format!(
                "{name}: {value}; expected {}..={} step {}",
                range.min, range.max, range.step
            )));
        }
        Ok(())
    }
    check(
        "center_frequency_hz",
        config.center_frequency_hz,
        &caps.frequency_hz,
    )?;
    check(
        "sample_rate_hz",
        config.sample_rate_hz.into(),
        &caps.sample_rate_hz,
    )?;
    if config.gains.len() != caps.gain_stages.len() {
        return Err(ControlError::Invalid(
            "provide exactly the reported gain stages".into(),
        ));
    }
    for stage in &caps.gain_stages {
        let value = config
            .gains
            .get(&stage.id)
            .ok_or_else(|| ControlError::Invalid(format!("missing gain {}", stage.id)))?;
        check(&stage.id, *value, &stage.range)?;
    }
    if !caps
        .baseband_filter_bandwidths_hz
        .contains(&config.baseband_filter_bandwidth_hz)
        || config.baseband_filter_bandwidth_hz > config.sample_rate_hz
    {
        return Err(ControlError::Invalid(
            "filter must be a reported bandwidth and no wider than sample rate".into(),
        ));
    }
    Ok(())
}

fn mock_selection() -> DeviceSelection {
    let source = MockSource::default();
    DeviceSelection {
        descriptor: source.descriptor(),
        capabilities: source.capabilities(),
        metadata: Default::default(),
        opened: false,
        supports_iq_streaming: true,
        configuration: None,
    }
}

enum Request {
    Shutdown(mpsc::Sender<Result<()>>),
    Inventory(mpsc::Sender<Result<DeviceInventory>>),
    Snapshot(mpsc::Sender<Result<DeviceSelection>>),
    Command(DeviceCommand, mpsc::Sender<Result<DeviceSelection>>),
}
/// Cloneable safe handle. Eight queued control requests maximum; saturation returns an error.
/// Native state is created, used and destroyed exclusively on one dedicated thread.
#[derive(Clone)]
pub struct DeviceController {
    sender: SyncSender<Request>,
}
impl DeviceController {
    pub fn new() -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(8);
        std::thread::Builder::new()
            .name("sdr-device-control".into())
            .spawn(move || {
                let mut worker = Worker::default();
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Shutdown(reply) => {
                            let result = worker.shutdown();
                            let _ = reply.send(result);
                            break;
                        }
                        Request::Inventory(reply) => {
                            let _ = reply.send(worker.inventory());
                        }
                        Request::Snapshot(reply) => {
                            let _ = reply.send(Ok(worker.snapshot()));
                        }
                        Request::Command(command, reply) => {
                            let _ = reply.send(worker.command(command));
                        }
                    }
                }
                // Receiver drops before library. RAII closes devices even on channel shutdown.
            })
            .map_err(|e| ControlError::Unavailable(e.to_string()))?;
        Ok(Self { sender })
    }
    fn call<T>(&self, make: impl FnOnce(mpsc::Sender<Result<T>>) -> Request) -> Result<T> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .try_send(make(tx))
            .map_err(|e| ControlError::Unavailable(e.to_string()))?;
        rx.recv()
            .map_err(|e| ControlError::Unavailable(e.to_string()))?
    }
    /// Closes the owned device and checks native library shutdown before returning.
    pub fn shutdown(&self) -> Result<()> {
        self.call(Request::Shutdown)
    }
    pub fn inventory(&self) -> Result<DeviceInventory> {
        self.call(Request::Inventory)
    }
    pub fn snapshot(&self) -> Result<DeviceSelection> {
        self.call(Request::Snapshot)
    }
    pub fn command(&self, command: DeviceCommand) -> Result<DeviceSelection> {
        self.call(|reply| Request::Command(command, reply))
    }
}

struct Worker {
    selected: DeviceSelection,
    receiver: Option<Box<dyn ReceiverControl>>,
    #[cfg(feature = "hackrf")]
    library: Option<std::rc::Rc<crate::hackrf::Library>>,
}
impl Default for Worker {
    fn default() -> Self {
        Self {
            selected: mock_selection(),
            receiver: None,
            #[cfg(feature = "hackrf")]
            library: None,
        }
    }
}
impl Worker {
    fn shutdown(&mut self) -> Result<()> {
        let close = self.command(DeviceCommand::Close).map(|_| ());
        #[cfg(feature = "hackrf")]
        if let Some(library) = self.library.take() {
            let library = std::rc::Rc::try_unwrap(library).map_err(|_| {
                ControlError::Conflict("native context still owned during shutdown".into())
            })?;
            library.shutdown()?;
        }
        close
    }

    #[cfg(feature = "hackrf")]
    fn library(&mut self) -> Result<std::rc::Rc<crate::hackrf::Library>> {
        if self.library.is_none() {
            self.library = Some(crate::hackrf::Library::new()?);
        }
        self.library
            .clone()
            .ok_or_else(|| ControlError::Unavailable("library initialization failed".into()))
    }
    fn inventory(&mut self) -> Result<DeviceInventory> {
        let devices = vec![mock_selection().descriptor];
        let mut result = DeviceInventory {
            devices,
            warnings: vec![],
        };
        #[cfg(feature = "hackrf")]
        match self.library().and_then(|lib| lib.enumerate()) {
            Ok(devices) => result.devices.extend(devices),
            Err(e) => result.warnings.push(e.to_string()),
        }
        #[cfg(not(feature = "hackrf"))]
        result
            .warnings
            .push("Hardware backend not compiled; enable the hackrf Cargo feature".into());
        Ok(result)
    }
    fn snapshot(&self) -> DeviceSelection {
        self.receiver
            .as_ref()
            .map_or_else(|| self.selected.clone(), |r| r.snapshot())
    }
    fn command(&mut self, command: DeviceCommand) -> Result<DeviceSelection> {
        match command {
            DeviceCommand::Select { id } => {
                if self.receiver.is_some() {
                    return Err(ControlError::Conflict(
                        "close the owned device before selecting another".into(),
                    ));
                }
                if id == "mock-0" {
                    self.selected = mock_selection();
                } else {
                    let inventory = self.inventory()?;
                    let descriptor = inventory
                        .devices
                        .into_iter()
                        .find(|d| d.id == id)
                        .ok_or_else(|| {
                            ControlError::Unavailable(format!(
                                "device {id} not enumerated; {}",
                                inventory.warnings.join("; ")
                            ))
                        })?;
                    // Exact board capabilities require opening: USB product ID alone cannot distinguish Pro.
                    self.selected = DeviceSelection {
                        descriptor,
                        capabilities: DeviceCapabilities {
                            frequency_hz: NumericRange {
                                min: 0,
                                max: 0,
                                step: 1,
                            },
                            sample_rate_hz: NumericRange {
                                min: 0,
                                max: 0,
                                step: 1,
                            },
                            gain_stages: vec![],
                            baseband_filter_bandwidths_hz: vec![],
                            supports_sweep: false,
                            max_sweep_ranges: None,
                        },
                        metadata: Default::default(),
                        opened: false,
                        supports_iq_streaming: false,
                        configuration: None,
                    };
                }
            }
            DeviceCommand::Open => {
                if self.selected.supports_iq_streaming {
                    return Err(ControlError::Invalid(
                        "mock uses the existing RX state API".into(),
                    ));
                }
                if self.receiver.is_none() {
                    #[cfg(feature = "hackrf")]
                    {
                        self.receiver = Some(Box::new(
                            self.library()?.open(&self.selected.descriptor.id)?,
                        ));
                    }
                    #[cfg(not(feature = "hackrf"))]
                    return Err(ControlError::Unavailable(
                        "hardware feature disabled".into(),
                    ));
                }
            }
            DeviceCommand::Close => {
                if let Some(mut receiver) = self.receiver.take() {
                    self.selected = receiver.snapshot();
                    self.selected.opened = false;
                    self.selected.configuration = None;
                    receiver.close()?;
                }
            }
            DeviceCommand::Configure { configuration } => {
                let receiver = self.receiver.as_mut().ok_or_else(|| {
                    ControlError::Conflict("open the selected device first".into())
                })?;
                validate_configuration(&receiver.snapshot().capabilities, &configuration)?;
                if let Err(error) = receiver.configure(configuration) {
                    // Native setters are not transactional. Close on partial failure; never report stale applied settings.
                    if let Some(mut receiver) = self.receiver.take() {
                        self.selected = receiver.snapshot();
                        self.selected.opened = false;
                        self.selected.configuration = None;
                        if let Err(close) = receiver.close() {
                            return Err(ControlError::Unavailable(format!(
                                "{error}; close also failed: {close}"
                            )));
                        }
                    }
                    return Err(error);
                }
            }
        }
        Ok(self.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn caps() -> DeviceCapabilities {
        DeviceCapabilities {
            frequency_hz: NumericRange {
                min: 100_000,
                max: 6_000_000_000,
                step: 1,
            },
            sample_rate_hz: NumericRange {
                min: 2_000_000,
                max: 20_000_000,
                step: 1,
            },
            gain_stages: vec![GainStage {
                id: "if".into(),
                label: "IF".into(),
                unit: "dB".into(),
                range: NumericRange {
                    min: 0,
                    max: 40,
                    step: 8,
                },
            }],
            baseband_filter_bandwidths_hz: vec![1_750_000, 5_000_000],
            supports_sweep: false,
            max_sweep_ranges: None,
        }
    }
    fn config() -> ReceiverConfiguration {
        ReceiverConfiguration {
            center_frequency_hz: 100_000_000,
            sample_rate_hz: 8_000_000,
            gains: [("if".into(), 16)].into(),
            baseband_filter_bandwidth_hz: 5_000_000,
        }
    }
    #[test]
    fn rejects_invalid_controls_before_native_calls() {
        let c = caps();
        let good = config();
        assert!(validate_configuration(&c, &good).is_ok());
        let mut v = good.clone();
        v.gains.insert("if".into(), 9);
        assert!(validate_configuration(&c, &v).is_err());
        let mut v = good.clone();
        v.gains.insert("unknown".into(), 0);
        assert!(validate_configuration(&c, &v).is_err());
        let mut v = good.clone();
        v.center_frequency_hz = 0;
        assert!(validate_configuration(&c, &v).is_err());
        let mut v = good.clone();
        v.sample_rate_hz = 1_000_000;
        assert!(validate_configuration(&c, &v).is_err());
        let mut v = good.clone();
        v.sample_rate_hz = 2_000_000;
        assert!(validate_configuration(&c, &v).is_err());
        let mut v = good;
        v.baseband_filter_bandwidth_hz = 4_000_001;
        assert!(validate_configuration(&c, &v).is_err());
    }
    #[test]
    fn mock_controller_does_not_initialize_hardware() {
        let c = DeviceController::new().unwrap();
        assert!(c.snapshot().unwrap().supports_iq_streaming);
        assert!(c
            .command(DeviceCommand::Configure {
                configuration: config()
            })
            .is_err());
        assert!(c.command(DeviceCommand::Close).is_ok());
        assert_eq!(
            c.command(DeviceCommand::Select {
                id: "mock-0".into()
            })
            .unwrap()
            .descriptor
            .id,
            "mock-0"
        );
    }
    struct FailingReceiver {
        closed: std::rc::Rc<std::cell::Cell<bool>>,
    }
    impl ReceiverControl for FailingReceiver {
        fn snapshot(&self) -> DeviceSelection {
            let mut s = mock_selection();
            s.capabilities = caps();
            s.opened = true;
            s.configuration = Some(config());
            s
        }
        fn configure(&mut self, _: ReceiverConfiguration) -> Result<()> {
            Err(ControlError::Unavailable("injected USB failure".into()))
        }
        fn close(&mut self) -> Result<()> {
            self.closed.set(true);
            Ok(())
        }
    }
    #[test]
    fn partial_native_failure_releases_ownership_and_clears_settings() {
        let closed = std::rc::Rc::new(std::cell::Cell::new(false));
        let mut w = Worker {
            receiver: Some(Box::new(FailingReceiver {
                closed: closed.clone(),
            })),
            ..Worker::default()
        };
        assert!(w
            .command(DeviceCommand::Select {
                id: "mock-0".into()
            })
            .is_err());
        assert!(w
            .command(DeviceCommand::Configure {
                configuration: config()
            })
            .is_err());
        assert!(closed.get());
        assert!(!w.snapshot().opened);
        assert!(w.snapshot().configuration.is_none());
    }
}
