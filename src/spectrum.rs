//! NoctaVox's spectrum display, ported to run on `realfft`.
//!
//! A 2048-point Hann window is transformed into ~100 log-spaced bands between
//! 20 Hz and 20 kHz. Every band is auto-gained against its own running peak
//! (instant attack, slow release), so quiet and loud passages both fill the
//! screen, and falls back by an exponential decay factor when the energy drops.
//! The bands are then resampled to the terminal width for drawing.
//!
//! Adapted from `Jaxx497/NoctaVox` (`src/visualization/spectrum.rs`, MIT); see
//! `THIRD_PARTY_NOTICES.md`.

use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// FFT window, and the number of recent samples the spectrum looks at.
pub const FFT_SIZE: usize = 2048;

/// Lowest and highest displayed frequency.
pub const MIN_HZ: f32 = 20.0;
pub const MAX_HZ: f32 = 20_000.0;

/// Default per-frame fall multiplier, matching NoctaVox's theme default.
pub const DEFAULT_DECAY: f32 = 0.85;

/// NoctaVox's spacing: each band is at least five percent wider than the last.
const BAND_RATIO: f32 = 1.05;

/// Smallest peak a band is normalized against, so silence cannot divide by zero.
const PEAK_FLOOR: f32 = 3e-3;

/// How much a band's running peak falls per analysed frame.
const PEAK_RELEASE: f32 = 0.99;

#[derive(Debug, Clone, Copy)]
struct Band {
    #[cfg_attr(not(test), allow(dead_code))]
    lo: f32,
    #[cfg_attr(not(test), allow(dead_code))]
    hi: f32,
    /// Inclusive FFT bin range.
    start: usize,
    /// Exclusive FFT bin range.
    end: usize,
}

/// Rolling spectrum analyser with the shaping used by NoctaVox.
pub struct Spectrum {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    history: VecDeque<f32>,
    bands: Vec<Band>,
    band_peaks: Vec<f32>,
    bins: Vec<f32>,
    display: Vec<f32>,
    decay: f32,
    sample_rate: u32,
}

impl Spectrum {
    pub fn new(decay: f32) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let output = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (TAU * i as f32 / FFT_SIZE as f32).cos()))
            .collect();

        Self {
            fft,
            window,
            input: vec![0.0; FFT_SIZE],
            output,
            scratch,
            history: VecDeque::with_capacity(FFT_SIZE),
            bands: Vec::new(),
            band_peaks: Vec::new(),
            bins: Vec::new(),
            display: Vec::new(),
            decay: clamp_decay(decay),
            sample_rate: 0,
        }
    }

    /// Append mono frames, keeping only the newest FFT-sized window.
    pub fn push(&mut self, samples: &[f32]) {
        if samples.len() >= FFT_SIZE {
            self.history.clear();
            self.history
                .extend(samples[samples.len() - FFT_SIZE..].iter().copied());
            return;
        }
        let excess = self.history.len().saturating_sub(FFT_SIZE - samples.len());
        self.history.drain(..excess);
        self.history.extend(samples.iter().copied());
    }

    /// Analyse the current window. Call once per rendered frame, like NoctaVox.
    pub fn update(&mut self, sample_rate: u32) {
        if sample_rate == 0 {
            return;
        }

        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            self.rebuild();
        }

        if self.history.len() < FFT_SIZE {
            self.release();
            return;
        }

        {
            let history = self.history.make_contiguous();
            for (input, (sample, window)) in self
                .input
                .iter_mut()
                .zip(history.iter().zip(self.window.iter()))
            {
                *input = sample * window;
            }
        }

        let processed =
            self.fft
                .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch);
        if processed.is_err() {
            self.release();
            return;
        }

        // realfft is unnormalized: a full-scale sine peaks near FFT_SIZE / 2.
        let scale = 2.0 / FFT_SIZE as f32;
        for (index, band) in self.bands.iter().enumerate() {
            let values = &self.output[band.start..band.end];
            let magnitude = if values.is_empty() {
                0.0
            } else {
                values.iter().map(|value| value.norm()).sum::<f32>() / values.len() as f32
            };
            let normalized = magnitude * scale;

            // Instant attack, slow release.
            if normalized > self.band_peaks[index] {
                self.band_peaks[index] = normalized;
            } else {
                self.band_peaks[index] = (self.band_peaks[index] * PEAK_RELEASE).max(1e-3);
            }

            let relative = (normalized / self.band_peaks[index]).clamp(0.0, 1.0);
            if relative > self.bins[index] {
                self.bins[index] = relative;
            } else {
                self.bins[index] *= self.decay;
            }
        }
    }

    /// Interpolate the bands down to `width` display columns.
    pub fn remap_display(&mut self, width: usize) {
        if width == 0 {
            return;
        }
        if self.bins.is_empty() {
            self.display.clear();
            return;
        }

        self.display.resize(width, 0.0);
        let last = self.bins.len() - 1;
        let denominator = (width - 1).max(1) as f32;
        for (column, out) in self.display.iter_mut().enumerate() {
            let source = (column as f32 / denominator) * last as f32;
            let lo = source.floor() as usize;
            let hi = (lo + 1).min(last);
            let fraction = source - lo as f32;
            *out = self.bins[lo] * (1.0 - fraction) + self.bins[hi] * fraction;
        }
    }

    /// The most recent `remap_display` result.
    pub fn display_bins(&self) -> &[f32] {
        &self.display
    }

    fn release(&mut self) {
        for bin in &mut self.bins {
            *bin *= self.decay;
        }
    }

    fn rebuild(&mut self) {
        let resolution = self.sample_rate as f32 / FFT_SIZE as f32;
        let mut bands = Vec::new();
        let mut freq = MIN_HZ;
        while freq < MAX_HZ {
            // At low frequencies a 5 % step is narrower than one FFT bin, so
            // widen to at least one bin until 5 % steps take over.
            let next = (freq * BAND_RATIO).max(freq + resolution);
            let hi = next.min(MAX_HZ);
            let start = (freq / resolution).ceil() as usize;
            let end = ((hi / resolution).ceil() as usize).min(self.output.len());
            bands.push(Band {
                lo: freq,
                hi,
                start: start.min(end),
                end,
            });
            freq = next;
        }

        let count = bands.len();
        self.bands = bands;
        if self.band_peaks.len() != count {
            self.band_peaks.resize(count, PEAK_FLOOR);
        }
        if self.bins.len() != count {
            self.bins.resize(count, 0.0);
        }
    }
}

fn clamp_decay(decay: f32) -> f32 {
    if decay.is_finite() {
        decay.clamp(0.7, 0.97)
    } else {
        DEFAULT_DECAY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frequency: f32, sample_rate: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (TAU * frequency * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn bands_tile_from_20_hz_to_20_khz() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&sine(1_000.0, 44_100, FFT_SIZE));
        spectrum.update(44_100);

        assert!(
            (50..=200).contains(&spectrum.bands.len()),
            "{} bands",
            spectrum.bands.len()
        );
        assert_eq!(spectrum.bands[0].lo, MIN_HZ);
        assert_eq!(spectrum.bands.last().unwrap().hi, MAX_HZ);
        for pair in spectrum.bands.windows(2) {
            assert_eq!(pair[0].hi, pair[1].lo, "bands must tile upward");
        }
        for band in &spectrum.bands {
            assert!(band.hi > band.lo);
            assert!(band.start <= band.end);
        }
    }

    #[test]
    fn finds_a_tone_and_decays_after_it_stops() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        let sample_rate = 44_100;
        spectrum.push(&sine(1_000.0, sample_rate, FFT_SIZE));
        spectrum.update(sample_rate);

        let tone_band = spectrum
            .bands
            .iter()
            .position(|band| band.lo <= 1_000.0 && 1_000.0 < band.hi)
            .expect("1 kHz band");
        let loudest = spectrum
            .bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("bins");

        assert!(
            (loudest as i32 - tone_band as i32).abs() <= 1,
            "loudest={loudest} tone={tone_band}"
        );
        assert!(spectrum.bins[tone_band] > 0.1);
        assert!(spectrum.bins[0] < 0.05, "no energy at 20 Hz");

        let before = spectrum.bins[tone_band];
        spectrum.push(&vec![0.0; FFT_SIZE]);
        spectrum.update(sample_rate);
        assert!((spectrum.bins[tone_band] - before * DEFAULT_DECAY).abs() < 1e-6);
    }

    #[test]
    fn remap_interpolates_across_the_width() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.bins = vec![0.0, 1.0];
        spectrum.remap_display(3);
        assert_eq!(spectrum.display_bins(), &[0.0, 0.5, 1.0]);
    }

    #[test]
    fn history_keeps_the_newest_window() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&vec![1.0; FFT_SIZE]);
        spectrum.push(&vec![2.0; 100]);
        assert_eq!(spectrum.history.len(), FFT_SIZE);
        // The oldest 100 samples were dropped; the newest 100 are all 2.0.
        assert_eq!(spectrum.history.front(), Some(&1.0));
        assert_eq!(spectrum.history.back(), Some(&2.0));
        assert_eq!(
            spectrum
                .history
                .iter()
                .filter(|sample| **sample == 2.0)
                .count(),
            100
        );

        spectrum.push(&vec![3.0; FFT_SIZE * 2]);
        assert_eq!(spectrum.history.len(), FFT_SIZE);
        assert!(spectrum.history.iter().all(|sample| *sample == 3.0));
    }

    #[test]
    fn decay_is_clamped_to_noctavox_range() {
        assert_eq!(Spectrum::new(2.0).decay, 0.97);
        assert_eq!(Spectrum::new(0.0).decay, 0.7);
        assert_eq!(Spectrum::new(f32::NAN).decay, DEFAULT_DECAY);
    }
}
