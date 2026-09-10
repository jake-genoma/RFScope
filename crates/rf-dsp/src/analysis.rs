//! Deterministic, uncalibrated measurements over FFT power bins.
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SpectrumMeasurements {
    pub peak_frequency_hz: f64,
    pub peak_dbfs: f32,
    pub noise_floor_dbfs: f32,
    pub snr_db: f32,
    pub bandwidth_3db_hz: f64,
    pub bandwidth_6db_hz: f64,
    pub occupied_bandwidth_99_hz: f64,
    pub amplitude_mean_dbfs: f32,
    pub amplitude_min_dbfs: f32,
    pub amplitude_max_dbfs: f32,
}

/// Measure frequency-domain bins in FFT-shifted order. Values are uncalibrated dBFS.
pub fn measure(
    bins_dbfs: &[f32],
    center_hz: u64,
    sample_rate_hz: u32,
) -> Option<SpectrumMeasurements> {
    if bins_dbfs.len() < 2 || sample_rate_hz == 0 {
        return None;
    }
    let peak_index = bins_dbfs
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?
        .0;
    let peak = bins_dbfs[peak_index];
    let mut sorted = bins_dbfs.to_vec();
    sorted.sort_by(f32::total_cmp);
    let noise = sorted[sorted.len() / 2];
    let bin_hz = sample_rate_hz as f64 / bins_dbfs.len() as f64;
    let edge = |drop: f32| {
        let threshold = peak - drop;
        let mut left = peak_index;
        while left > 0 && bins_dbfs[left - 1] >= threshold {
            left -= 1;
        }
        let mut right = peak_index;
        while right + 1 < bins_dbfs.len() && bins_dbfs[right + 1] >= threshold {
            right += 1;
        }
        (right - left + 1) as f64 * bin_hz
    };
    let mut power: Vec<f64> = bins_dbfs
        .iter()
        .map(|v| 10f64.powf(*v as f64 / 10.0))
        .collect();
    let total: f64 = power.iter().sum();
    power.sort_by(f64::total_cmp);
    let target = total * 0.99;
    let mut cumulative = 0.0;
    let mut count = 0usize;
    for value in power.into_iter().rev() {
        cumulative += value;
        count += 1;
        if cumulative >= target {
            break;
        }
    }
    let mean = bins_dbfs.iter().sum::<f32>() / bins_dbfs.len() as f32;
    let min = *sorted.first()?;
    let max = *sorted.last()?;
    Some(SpectrumMeasurements {
        peak_frequency_hz: center_hz as f64
            + (peak_index as f64 - bins_dbfs.len() as f64 / 2.0) * bin_hz,
        peak_dbfs: peak,
        noise_floor_dbfs: noise,
        snr_db: peak - noise,
        bandwidth_3db_hz: edge(3.0),
        bandwidth_6db_hz: edge(6.0),
        occupied_bandwidth_99_hz: count as f64 * bin_hz,
        amplitude_mean_dbfs: mean,
        amplitude_min_dbfs: min,
        amplitude_max_dbfs: max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measures_peak_noise_and_bandwidth() {
        let mut bins = vec![-80.0; 1024];
        for bin in bins.iter_mut().take(516).skip(508) {
            *bin = -3.0;
        }
        bins[512] = -1.0;
        let m = measure(&bins, 100_000_000, 1_024_000).unwrap();
        assert_eq!(m.peak_frequency_hz, 100_000_000.0);
        assert!((m.noise_floor_dbfs + 80.0).abs() < 0.01);
        assert!((m.snr_db - 79.0).abs() < 0.01);
        assert!(m.bandwidth_3db_hz >= 8_000.0);
        assert!(m.bandwidth_6db_hz >= 8_000.0);
        assert!(m.occupied_bandwidth_99_hz > 0.0);
    }
}
