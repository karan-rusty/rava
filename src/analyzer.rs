//! Frequency-band analyser for the wide bar visualizer.

use crate::dsp::{Fft, History};

const FFT_SIZE: usize = 2048;
const BARS: usize = 32;
const FLOOR_DB: f32 = -70.0;
const DYNAMIC_RANGE_DB: f32 = -FLOOR_DB;

pub struct Analyzer {
    fft: Fft,
    history: History,
    bands: Vec<(usize, usize)>,
    values: Vec<f32>,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            fft: Fft::new(FFT_SIZE),
            history: History::new(FFT_SIZE),
            bands: make_bands(sample_rate),
            values: vec![0.0; BARS],
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.history.push(samples);
    }

    pub fn update(&mut self, dt: f32) {
        let spectrum = self.fft.process(self.history.samples());
        let scale = 2.0 / FFT_SIZE as f32;
        for (i, (start, end)) in self.bands.iter().enumerate() {
            let end = (*end).min(spectrum.len());
            let start = (*start).min(end.saturating_sub(1));
            let mean = spectrum[start..end]
                .iter()
                .map(|value| value.norm())
                .sum::<f32>()
                / (end - start).max(1) as f32;
            let db = 20.0 * (mean * scale + 1e-9).log10();
            let target = ((db - FLOOR_DB) / DYNAMIC_RANGE_DB).clamp(0.0, 1.0);
            let fall = dt * 1.6;
            let current = self.values[i];
            self.values[i] = if target > current {
                target
            } else {
                (current - fall).max(target)
            };
        }
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

fn make_bands(sample_rate: u32) -> Vec<(usize, usize)> {
    let lower = 50.0f32;
    let higher = 12_000.0f32;
    let ratio = (higher / lower).powf(1.0 / BARS as f32);
    let bins_per_hz = FFT_SIZE as f32 / sample_rate as f32;
    (0..BARS)
        .map(|i| {
            let f0 = lower * ratio.powi(i as i32);
            let f1 = lower * ratio.powi(i as i32 + 1);
            let start = ((f0 * bins_per_hz) as usize).clamp(1, FFT_SIZE / 2);
            let end = (((f1 * bins_per_hz) as usize).max(start + 1)).min(FFT_SIZE / 2 + 1);
            (start, end)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn sine(frequency: f32, sample_rate: u32) -> Vec<f32> {
        (0..FFT_SIZE)
            .map(|i| (TAU * frequency * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    fn tone_band(frequency: f32) -> usize {
        let resolution = 44_100.0 / FFT_SIZE as f32;
        make_bands(44_100)
            .iter()
            .position(|(start, end)| {
                *start as f32 * resolution <= frequency && frequency < *end as f32 * resolution
            })
            .expect("frequency is covered by a band")
    }

    #[test]
    fn a_tone_lights_the_band_that_contains_it() {
        let mut analyzer = Analyzer::new(44_100);
        analyzer.push(&sine(1_000.0, 44_100));
        analyzer.update(0.016);

        let band = tone_band(1_000.0);
        let loudest = analyzer
            .values()
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("values");
        assert!(
            (loudest as i32 - band as i32).abs() <= 1,
            "loudest={loudest} tone={band}"
        );
        assert!(analyzer.values()[band] > 0.5);
    }

    #[test]
    fn values_fall_after_the_tone_stops() {
        let mut analyzer = Analyzer::new(44_100);
        analyzer.push(&sine(1_000.0, 44_100));
        analyzer.update(0.016);
        let before = analyzer.values()[tone_band(1_000.0)];
        assert!(before > 0.5);

        analyzer.push(&[0.0; FFT_SIZE]);
        analyzer.update(0.05);
        assert!(analyzer.values()[tone_band(1_000.0)] < before);
    }
}
