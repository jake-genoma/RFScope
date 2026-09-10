//! Explicit hardware-only command; never executed by cargo test.
use rf_device::control::DeviceController;
use rf_types::{DeviceCommand, ReceiverConfiguration};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let serial = std::env::args()
        .nth(1)
        .ok_or("usage: hackrf-diagnostic <full-serial>; no transmission or IQ streaming")?;
    let controller = DeviceController::new()?;
    println!("Inventory: {:#?}", controller.inventory()?);
    controller.command(DeviceCommand::Select {
        id: format!("hackrf:{serial}"),
    })?;
    let opened = controller.command(DeviceCommand::Open)?;
    println!("Opened / metadata: {opened:#?}");
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        for (frequency, rate, filter, lna, vga) in [
            (100_000_000, 8_000_000, 5_000_000, 16, 20),
            (101_000_000, 10_000_000, 7_000_000, 24, 24),
        ] {
            let configuration = ReceiverConfiguration {
                center_frequency_hz: frequency,
                sample_rate_hz: rate,
                baseband_filter_bandwidth_hz: filter,
                gains: [
                    ("if".into(), lna),
                    ("baseband".into(), vga),
                    ("rf_amp".into(), 0),
                ]
                .into(),
            };
            let applied = controller.command(DeviceCommand::Configure {
                configuration: configuration.clone(),
            })?;
            if applied.configuration.as_ref() != Some(&configuration) {
                return Err("applied configuration mismatch".into());
            }
            println!("Native setters succeeded (not RF readback): {configuration:?}");
        }
        Ok(())
    })();
    let closed = controller.command(DeviceCommand::Close)?;
    if closed.opened {
        return Err("close did not release ownership".into());
    }
    println!("Closed cleanly");
    result?;
    controller.command(DeviceCommand::Open)?;
    controller.command(DeviceCommand::Close)?;
    controller.shutdown()?;
    println!("Reopen / close and libhackrf_exit succeeded; diagnostic PASS");
    Ok(())
}
