//! Minimal ABI declarations from official libhackrf 2026.01.3 hackrf.h.
//! No RX callbacks, TX functions, or raw pointers are exposed outside this backend.
use std::ffi::{c_char, c_int, c_void};
#[repr(C)]
pub struct Device {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct DeviceList {
    pub serial_numbers: *mut *mut c_char,
    pub usb_board_ids: *mut c_int,
    pub usb_device_index: *mut c_int,
    pub devicecount: c_int,
    pub usb_devices: *mut *mut c_void,
    pub usb_devicecount: c_int,
}
#[repr(C)]
#[derive(Default)]
pub struct PartSerial {
    pub part_id: [u32; 2],
    pub serial_no: [u32; 4],
}
extern "C" {
    pub fn hackrf_init() -> c_int;
    pub fn hackrf_exit() -> c_int;
    pub fn hackrf_library_version() -> *const c_char;
    pub fn hackrf_library_release() -> *const c_char;
    pub fn hackrf_error_name(code: c_int) -> *const c_char;
    pub fn hackrf_device_list() -> *mut DeviceList;
    pub fn hackrf_device_list_free(list: *mut DeviceList);
    pub fn hackrf_open_by_serial(serial: *const c_char, device: *mut *mut Device) -> c_int;
    pub fn hackrf_close(device: *mut Device) -> c_int;
    pub fn hackrf_board_id_read(device: *mut Device, id: *mut u8) -> c_int;
    pub fn hackrf_board_id_name(id: c_int) -> *const c_char;
    pub fn hackrf_version_string_read(
        device: *mut Device,
        version: *mut c_char,
        length: u8,
    ) -> c_int;
    pub fn hackrf_usb_api_version_read(device: *mut Device, version: *mut u16) -> c_int;
    pub fn hackrf_board_rev_read(device: *mut Device, revision: *mut u8) -> c_int;
    pub fn hackrf_board_rev_name(revision: c_int) -> *const c_char;
    pub fn hackrf_board_partid_serialno_read(device: *mut Device, serial: *mut PartSerial)
        -> c_int;
    pub fn hackrf_set_freq(device: *mut Device, hz: u64) -> c_int;
    pub fn hackrf_set_sample_rate(device: *mut Device, hz: f64) -> c_int;
    pub fn hackrf_set_lna_gain(device: *mut Device, db: u32) -> c_int;
    pub fn hackrf_set_vga_gain(device: *mut Device, db: u32) -> c_int;
    pub fn hackrf_set_amp_enable(device: *mut Device, enabled: u8) -> c_int;
    pub fn hackrf_set_antenna_enable(device: *mut Device, enabled: u8) -> c_int;
    pub fn hackrf_set_baseband_filter_bandwidth(device: *mut Device, hz: u32) -> c_int;
    pub fn hackrf_compute_baseband_filter_bw_round_down_lt(hz: u32) -> u32;
}
