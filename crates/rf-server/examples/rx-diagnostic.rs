//! Explicit RX-only hardware throughput test. No USB access in ordinary tests.
use rf_device::control::DeviceController;
use rf_engine::Engine;
use rf_types::DeviceCommand;
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let serial = std::env::args()
        .nth(1)
        .ok_or("provide the full device serial")?;
    let seconds: u64 = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "15".into())
        .parse()?;
    let devices = DeviceController::new()?;
    devices.command(DeviceCommand::Select {
        id: format!("hackrf:{serial}"),
    })?;
    let opened = devices.command(DeviceCommand::Open)?;
    println!(
        "device={:?} metadata={:?}",
        opened.descriptor, opened.metadata
    );
    let mut config = opened.configuration.ok_or("missing configuration")?;
    let engine = Engine::mock();
    engine.hardware.store(true, Ordering::Release);
    let task = tokio::spawn(engine.clone().run(devices.clone()));
    // Subscribe so display-no-subscriber metrics do not obscure source drops.
    let mut frames = engine.frames.subscribe();
    for rate in [2_000_000, 8_000_000, 10_000_000, 20_000_000] {
        config.sample_rate_hz = rate;
        config.baseband_filter_bandwidth_hz = if rate == 2_000_000 {
            1_750_000
        } else {
            5_000_000
        };
        devices.command(DeviceCommand::Configure {
            configuration: config.clone(),
        })?;
        devices.command(DeviceCommand::Start)?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let before = engine.diagnostics();
        let started = Instant::now();
        let deadline = started + Duration::from_secs(seconds);
        let mut observed = 0;
        let mut finite = true;
        while Instant::now() < deadline {
            if let Ok(Ok(frame)) =
                tokio::time::timeout(Duration::from_millis(500), frames.recv()).await
            {
                if &frame[..4] != b"RFSP" {
                    return Err("invalid spectrum magic".into());
                }
                for bin in frame[48..].as_chunks::<4>().0 {
                    finite &= f32::from_le_bytes(*bin).is_finite();
                }
                observed += 1;
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        let after = engine.diagnostics();
        println!("rate={rate} elapsed={elapsed:.3} native_MSps={:.4} consumed_MSps={:.4} dropped_blocks={} dropped_bytes={} fft_fps={:.2} observed_frames={observed} finite={finite} native_bytes={}",
            (after.received_bytes-before.received_bytes) as f64 / 2e6 / elapsed,
            (after.received_samples-before.received_samples) as f64 / 1e6 / elapsed,
            after.dropped_iq_blocks-before.dropped_iq_blocks,
            after.dropped_iq_bytes-before.dropped_iq_bytes,
            (after.fft_frames-before.fft_frames) as f64 / elapsed,
            after.received_bytes-before.received_bytes);
        if !finite || observed == 0 || !devices.snapshot()?.running {
            return Err("no healthy live spectrum".into());
        }
        // Active tuning/reconfiguration must restart RX and emit correctly labelled spectra.
        config.center_frequency_hz += 100_000;
        devices.command(DeviceCommand::Configure {
            configuration: config.clone(),
        })?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        if !devices.snapshot()?.running {
            return Err("reconfiguration lost RX".into());
        }
        devices.command(DeviceCommand::Stop)?;
        devices.command(DeviceCommand::Stop)?;
        if devices.snapshot()?.running {
            return Err("RX still running after stop".into());
        }
        println!("retune_restart_and_idempotent_stop=passed rate={rate}");
    }
    for _ in 0..5 {
        devices.command(DeviceCommand::Start)?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        devices.command(DeviceCommand::Stop)?;
    }
    devices.command(DeviceCommand::Close)?;
    devices.command(DeviceCommand::Open)?;
    devices.command(DeviceCommand::Start)?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    devices.command(DeviceCommand::Close)?;
    engine.shutdown.store(true, Ordering::Release);
    task.await?;
    devices.shutdown()?;
    println!("five_restart_cycles_close_while_running_reopen_library_shutdown=passed");
    Ok(())
}
