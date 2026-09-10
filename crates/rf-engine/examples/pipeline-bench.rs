//! Lightweight reproducible release microbenchmark; not a statistical benchmark suite.
use rf_device::{
    stream::{BufferedSource, BLOCK_BYTES},
    IqSource, MockSource,
};
use rf_dsp::{SpectrumAnalyzer, Window};
use std::{hint::black_box, sync::atomic::Ordering, time::Instant};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mock = MockSource::default();
    let mut source = BufferedSource::new(mock.descriptor(), mock.capabilities(), mock.state());
    source
        .ingress
        .counters
        .running
        .store(true, Ordering::Release);
    let input = vec![37; BLOCK_BYTES];
    let mut output = vec![Default::default(); BLOCK_BYTES / 2];
    let start = Instant::now();
    for _ in 0..2000 {
        source.ingress.push(black_box(&input));
        source.read(black_box(&mut output)).await?;
    }
    println!(
        "bounded_copy_and_signed_conversion_MSps={:.2}",
        2000.0 * (BLOCK_BYTES / 2) as f64 / start.elapsed().as_secs_f64() / 1e6
    );
    for size in [2048, 8192, 65536] {
        let mut fft = SpectrumAnalyzer::new(size, Window::BlackmanHarris)?;
        let mut bins = Vec::new();
        let start = Instant::now();
        for _ in 0..1000 {
            fft.process(black_box(&output[..size]), black_box(&mut bins))?;
        }
        println!(
            "fft_size={size} mean_us={:.2}",
            start.elapsed().as_secs_f64() * 1e3
        );
    }
    for rate in [2_000_000, 20_000_000] {
        let mut channel = rf_dsp::channel::Channelizer::new(rate, 100_000.0, 12_000)?;
        let mut narrow = Vec::new();
        let start = Instant::now();
        for _ in 0..100 {
            channel.process(black_box(&output), black_box(&mut narrow));
        }
        println!(
            "channel_input_rate={rate} channel_MSps={:.2}",
            100.0 * output.len() as f64 / start.elapsed().as_secs_f64() / 1e6
        );
    }
    Ok(())
}
