//! Allocation-conscious FFT/PSD processing.
pub mod audio;
pub mod channel;
pub mod demod;
use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::{f32::consts::PI, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub enum Window {
    Rectangular,
    Hann,
    Hamming,
    Blackman,
    BlackmanHarris,
}
pub struct SpectrumAnalyzer {
    size: usize,
    fft: Arc<dyn Fft<f32>>,
    work: Vec<Complex32>,
    window: Vec<f32>,
    coherent_gain: f32,
}
impl SpectrumAnalyzer {
    pub fn new(size: usize, kind: Window) -> Result<Self, &'static str> {
        if size < 2 || !size.is_power_of_two() {
            return Err("FFT size must be a power of two >= 2");
        }
        let window: Vec<f32> = (0..size)
            .map(|n| {
                let x = 2.0 * PI * n as f32 / (size - 1) as f32;
                match kind {
                    Window::Rectangular => 1.0,
                    Window::Hann => 0.5 - 0.5 * x.cos(),
                    Window::Hamming => 0.54 - 0.46 * x.cos(),
                    Window::Blackman => 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos(),
                    Window::BlackmanHarris => {
                        0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos()
                            - 0.01168 * (3.0 * x).cos()
                    }
                }
            })
            .collect();
        let coherent_gain = window.iter().sum::<f32>() / size as f32;
        let fft = FftPlanner::new().plan_fft_forward(size);
        Ok(Self {
            size,
            fft,
            work: vec![Complex32::default(); size],
            window,
            coherent_gain,
        })
    }
    pub fn size(&self) -> usize {
        self.size
    }
    pub fn process(
        &mut self,
        input: &[Complex32],
        output: &mut Vec<f32>,
    ) -> Result<(), &'static str> {
        if input.len() != self.size {
            return Err("input length differs from FFT size");
        }
        for ((dst, src), w) in self.work.iter_mut().zip(input).zip(&self.window) {
            *dst = *src * *w;
        }
        self.fft.process(&mut self.work);
        output.resize(self.size, 0.0);
        let scale = (self.size as f32 * self.coherent_gain).powi(2);
        for (i, out) in output.iter_mut().enumerate() {
            let shifted = (i + self.size / 2) % self.size;
            *out = 10.0 * (self.work[shifted].norm_sqr() / scale).max(1e-12).log10();
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peak_is_shifted_into_frequency_order() {
        let n = 1024;
        let mut input = Vec::new();
        for i in 0..n {
            let p = 2.0 * PI * 128.0 * i as f32 / n as f32;
            input.push(Complex32::from_polar(1.0, p));
        }
        let mut a = SpectrumAnalyzer::new(n, Window::Rectangular).unwrap();
        let mut out = vec![];
        a.process(&input, &mut out).unwrap();
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak, 640);
        assert!(out[peak] > -0.01);
    }
}
