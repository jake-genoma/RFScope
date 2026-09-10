//! Safe, thread-confined ownership over the official C library. All unsafe use stays here.
mod ffi;
use crate::control::{validate_configuration, ControlError, ReceiverControl, Result};
use rf_types::*;
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    ptr::NonNull,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

// libhackrf itself has process-global state. This guard only enforces one explicit
// Library owner; it holds no device/configuration state. Rc keeps owners !Send/!Sync.
static INITIALIZED: AtomicBool = AtomicBool::new(false);
pub(crate) struct Library {
    active: bool,
    _thread_bound: std::marker::PhantomData<Rc<()>>,
}
fn check(operation: &'static str, code: i32) -> Result<()> {
    if code == 0 {
        return Ok(());
    }
    // SAFETY: libhackrf returns a static NUL-terminated error name for any error code.
    let message = unsafe { string(ffi::hackrf_error_name(code)) };
    Err(ControlError::Native {
        operation,
        code,
        message,
    })
}
// SAFETY contract: caller supplies a live NUL-terminated C string or NULL.
unsafe fn string(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        return "unavailable".into();
    }
    // SAFETY: guaranteed by this function's callers / libhackrf string API contracts.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
impl Library {
    pub(crate) fn new() -> Result<Rc<Self>> {
        if INITIALIZED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(ControlError::Conflict(
                "libhackrf already has an owner in this process".into(),
            ));
        }
        // SAFETY: process lifecycle guard excludes another initializer or active context.
        if let Err(e) = check("hackrf_init", unsafe { ffi::hackrf_init() }) {
            INITIALIZED.store(false, Ordering::Release);
            return Err(e);
        }
        Ok(Rc::new(Self {
            active: true,
            _thread_bound: std::marker::PhantomData,
        }))
    }
    pub(crate) fn enumerate(&self) -> Result<Vec<DeviceDescriptor>> {
        // SAFETY: initialized context remains alive; list is owned until guard drop.
        let ptr = NonNull::new(unsafe { ffi::hackrf_device_list() })
            .ok_or_else(|| ControlError::Unavailable("hackrf_device_list returned NULL".into()))?;
        struct List(NonNull<ffi::DeviceList>);
        impl Drop for List {
            fn drop(&mut self) {
                /* SAFETY: list allocated by libhackrf, freed exactly once. */
                unsafe { ffi::hackrf_device_list_free(self.0.as_ptr()) };
            }
        }
        let list = List(ptr);
        // SAFETY: live list and its devicecount entries are library-owned, read-only arrays.
        let raw = unsafe { list.0.as_ref() };
        if raw.devicecount < 0 || (raw.devicecount > 0 && raw.serial_numbers.is_null()) {
            return Err(ControlError::Unavailable(
                "invalid native device list".into(),
            ));
        }
        let mut devices = vec![];
        for i in 0..raw.devicecount as usize {
            // SAFETY: i is within devicecount; serial is either NULL or a live C string.
            let serial = unsafe { *raw.serial_numbers.add(i) };
            if serial.is_null() {
                return Err(ControlError::Unavailable(
                    "HackRF serial unavailable; refusing ambiguous index-based ownership".into(),
                ));
            }
            // SAFETY: string belongs to the list and is copied before list drop.
            let serial = unsafe { string(serial) };
            devices.push(DeviceDescriptor {
                id: format!("hackrf:{serial}"),
                name: format!("HackRF USB device {serial}"),
                driver: "hackrf".into(),
            });
        }
        Ok(devices)
    }
    pub(crate) fn open(self: &Rc<Self>, id: &str) -> Result<Receiver> {
        // Require a complete enumerated serial; libhackrf otherwise accepts suffix matches.
        if !self.enumerate()?.iter().any(|d| d.id == id) {
            return Err(ControlError::Unavailable(format!(
                "{id} no longer enumerated"
            )));
        }
        let serial = id
            .strip_prefix("hackrf:")
            .ok_or_else(|| ControlError::Invalid("invalid device ID".into()))?;
        let serial_c = CString::new(serial).map_err(|e| ControlError::Invalid(e.to_string()))?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: valid C serial and writable output pointer; context initialized.
        check("hackrf_open_by_serial", unsafe {
            ffi::hackrf_open_by_serial(serial_c.as_ptr(), &mut raw)
        })?;
        let pointer = NonNull::new(raw)
            .ok_or_else(|| ControlError::Unavailable("open succeeded with NULL handle".into()))?;
        let mut receiver = Receiver {
            pointer: Some(pointer),
            _library: self.clone(),
            selection: None,
        };
        receiver.selection = Some(receiver.query(id)?);
        // Establish explicit safe defaults, including antenna power disabled. No RX/TX start is bound.
        let config = ReceiverConfiguration {
            center_frequency_hz: 100_000_000,
            sample_rate_hz: 8_000_000,
            gains: [
                ("rf_amp".into(), 0),
                ("if".into(), 0),
                ("baseband".into(), 0),
            ]
            .into(),
            baseband_filter_bandwidth_hz: 5_000_000,
        };
        receiver.configure(config)?;
        Ok(receiver)
    }
}
impl Library {
    pub(crate) fn shutdown(mut self) -> Result<()> {
        self.finish()
    }
    fn finish(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        // SAFETY: only called when the final Rc has gone, after all receivers close.
        let result = check("hackrf_exit", unsafe { ffi::hackrf_exit() });
        self.active = false;
        // If native shutdown fails, do not permit another context over uncertain native state.
        if result.is_ok() {
            INITIALIZED.store(false, Ordering::Release);
        }
        result
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            eprintln!("{error}");
        }
    }
}
pub(crate) struct Receiver {
    pointer: Option<NonNull<ffi::Device>>,
    _library: Rc<Library>,
    selection: Option<DeviceSelection>,
}
impl Receiver {
    fn pointer(&self) -> Result<*mut ffi::Device> {
        self.pointer
            .map(NonNull::as_ptr)
            .ok_or_else(|| ControlError::Conflict("device is closed".into()))
    }
    fn query(&self, id: &str) -> Result<DeviceSelection> {
        let p = self.pointer()?;
        let mut board = 255;
        let mut api = 0;
        let mut firmware = [0u8; 256];
        let mut serial = ffi::PartSerial::default();
        // SAFETY: p is exclusively owned and live; all output buffers match header sizes.
        unsafe {
            check(
                "hackrf_board_id_read",
                ffi::hackrf_board_id_read(p, &mut board),
            )?;
            check(
                "hackrf_usb_api_version_read",
                ffi::hackrf_usb_api_version_read(p, &mut api),
            )?;
            check(
                "hackrf_version_string_read",
                ffi::hackrf_version_string_read(p, firmware.as_mut_ptr().cast(), 255),
            )?;
            check(
                "hackrf_board_partid_serialno_read",
                ffi::hackrf_board_partid_serialno_read(p, &mut serial),
            )?;
        }
        let mut metadata = BTreeMap::new();
        // SAFETY: board/library names are static C strings; firmware buffer has a trailing zero.
        let name = unsafe {
            metadata.insert(
                "library_version".into(),
                string(ffi::hackrf_library_version()),
            );
            metadata.insert(
                "library_release".into(),
                string(ffi::hackrf_library_release()),
            );
            metadata.insert("firmware_version".into(), string(firmware.as_ptr().cast()));
            string(ffi::hackrf_board_id_name(board.into()))
        };
        metadata.insert("board_id".into(), board.to_string());
        metadata.insert(
            "firmware_api".into(),
            format!("{:x}.{:02x}", api >> 8, api & 255),
        );
        metadata.insert(
            "serial_number".into(),
            serial
                .serial_no
                .iter()
                .map(|v| format!("{v:08x}"))
                .collect(),
        );
        metadata.insert(
            "part_id".into(),
            format!("{:08x} {:08x}", serial.part_id[0], serial.part_id[1]),
        );
        if api >= 0x0106 {
            let mut revision = 255;
            // SAFETY: live device and writable u8, supported API; name is a static C string.
            unsafe {
                check(
                    "hackrf_board_rev_read",
                    ffi::hackrf_board_rev_read(p, &mut revision),
                )?;
                metadata.insert(
                    "hardware_revision".into(),
                    string(ffi::hackrf_board_rev_name(revision.into())),
                );
            }
        }
        metadata.insert("capability_source".into(),"Board-specific documented compatibility profile; libhackrf has no range query. Filters from library helper. Rates limited to 2–20 MS/s; no extended sample modes.".into());
        Ok(DeviceSelection {
            descriptor: DeviceDescriptor {
                id: id.into(),
                name,
                driver: "hackrf".into(),
            },
            capabilities: capabilities(board)?,
            metadata,
            opened: true,
            supports_iq_streaming: false,
            configuration: None,
        })
    }
}
impl ReceiverControl for Receiver {
    fn snapshot(&self) -> DeviceSelection {
        // Impossibility: only Library::open constructs Receiver; it sets selection before returning it.
        self.selection
            .clone()
            .expect("successfully opened receiver has metadata")
    }
    fn configure(&mut self, config: ReceiverConfiguration) -> Result<()> {
        let selection = self
            .selection
            .as_ref()
            .ok_or_else(|| ControlError::Unavailable("metadata not initialized".into()))?;
        validate_configuration(&selection.capabilities, &config)?;
        let p = self.pointer()?;
        // SAFETY: exclusive live handle; every argument validated against backend capabilities.
        // Rate first (resets filter), then re-tune, then explicit filter. No streaming is active.
        unsafe {
            check(
                "hackrf_set_antenna_enable(off)",
                ffi::hackrf_set_antenna_enable(p, 0),
            )?;
            check(
                "hackrf_set_sample_rate",
                ffi::hackrf_set_sample_rate(p, config.sample_rate_hz.into()),
            )?;
            check(
                "hackrf_set_freq",
                ffi::hackrf_set_freq(p, config.center_frequency_hz),
            )?;
            check(
                "hackrf_set_lna_gain",
                ffi::hackrf_set_lna_gain(p, config.gains["if"] as u32),
            )?;
            check(
                "hackrf_set_vga_gain",
                ffi::hackrf_set_vga_gain(p, config.gains["baseband"] as u32),
            )?;
            check(
                "hackrf_set_amp_enable",
                ffi::hackrf_set_amp_enable(p, config.gains["rf_amp"] as u8),
            )?;
            check(
                "hackrf_set_baseband_filter_bandwidth",
                ffi::hackrf_set_baseband_filter_bandwidth(p, config.baseband_filter_bandwidth_hz),
            )?;
        }
        if let Some(selection) = self.selection.as_mut() {
            selection.configuration = Some(config);
        }
        Ok(())
    }
    fn close(&mut self) -> Result<()> {
        if let Some(pointer) = self.pointer.take() {
            // SAFETY: unique handle consumed exactly once; libhackrf frees it even if teardown reports error.
            check("hackrf_close", unsafe {
                ffi::hackrf_close(pointer.as_ptr())
            })?;
        }
        Ok(())
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        if let Err(e) = self.close() {
            eprintln!("{e}");
        }
    }
}
fn capabilities(board: u8) -> Result<DeviceCapabilities> {
    let minimum = match board {
        5 => 100_000,
        2 | 4 => 1_000_000,
        _ => {
            return Err(ControlError::Unavailable(format!(
                "board ID {board} has no reviewed RX capability profile"
            )))
        }
    };
    // Walk the library's strict round-down helper from its maximum; no duplicated filter table.
    let mut filters = vec![];
    let mut request = u32::MAX;
    loop {
        // SAFETY: pure library helper, no pointers or device state.
        let value = unsafe { ffi::hackrf_compute_baseband_filter_bw_round_down_lt(request) };
        if filters.last() == Some(&value) || value == 0 {
            break;
        }
        filters.push(value);
        request = value;
    }
    filters.reverse();
    Ok(DeviceCapabilities {
        frequency_hz: NumericRange {
            min: minimum,
            max: 6_000_000_000,
            step: 1,
        },
        sample_rate_hz: NumericRange {
            min: 2_000_000,
            max: 20_000_000,
            step: 1,
        },
        gain_stages: vec![
            GainStage {
                id: "if".into(),
                label: "LNA / IF".into(),
                unit: "dB".into(),
                range: NumericRange {
                    min: 0,
                    max: 40,
                    step: 8,
                },
            },
            GainStage {
                id: "baseband".into(),
                label: "VGA / baseband".into(),
                unit: "dB".into(),
                range: NumericRange {
                    min: 0,
                    max: 62,
                    step: 2,
                },
            },
            GainStage {
                id: "rf_amp".into(),
                label: "RF amplifier".into(),
                unit: "enabled (0/1)".into(),
                range: NumericRange {
                    min: 0,
                    max: 1,
                    step: 1,
                },
            },
        ],
        baseband_filter_bandwidths_hz: filters,
        supports_sweep: false,
        max_sweep_ranges: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_filter_table_and_board_profiles_need_no_usb() {
        let caps = capabilities(5).unwrap();
        assert_eq!(caps.frequency_hz.min, 100_000);
        assert!(caps.baseband_filter_bandwidths_hz.contains(&5_000_000));
        assert!(caps.baseband_filter_bandwidths_hz.contains(&7_000_000));
        assert!(caps
            .baseband_filter_bandwidths_hz
            .windows(2)
            .all(|v| v[0] < v[1]));
        assert_eq!(capabilities(2).unwrap().frequency_hz.min, 1_000_000);
        assert!(capabilities(255).is_err());
    }
}
