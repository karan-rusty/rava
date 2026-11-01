//! A NoctaVox-style braille spectrum with a true frequency axis.
//!
//! The Hann window, per-band auto-gain, logarithmic band layout, decay and
//! braille presentation come from NoctaVox (`src/visualization/spectrum.rs`,
//! MIT; see `THIRD_PARTY_NOTICES.md`). Each band is normalized against its own
//! running peak, so quiet parts of a mix rise to fill their bands instead of
//! collapsing next to the kick drum and the display keeps the shape of the
//! music.
//!
//! One detail is deliberately different: NoctaVox spreads its `bands` vector
//! evenly over the display. That vector is only roughly logarithmic: below
//! ~430 Hz every band is one FFT bin wide, so 1 kHz lands well left of where a
//! logarithmic axis puts it. This version maps each display column to
//! 20 Hz..20 kHz logarithmically and interpolates the band values at that
//! frequency.

use crate::dsp::{Fft, History};

/// FFT window, and the number of recent samples the spectrum looks at.
///
/// NoctaVox uses 2048; 4096 halves the bin width so the 20 Hz..20 kHz bands
/// stay distinct down into the bass instead of collapsing into one-bin bands.
const FFT_SIZE: usize = 4096;

/// Lowest and highest displayed frequency.
const MIN_HZ: f32 = 20.0;
const MAX_HZ: f32 = 20_000.0;

/// Default per-frame fall multiplier, matching NoctaVox's theme default.
pub const DEFAULT_DECAY: f32 = 0.85;

/// NoctaVox's spacing: each band is at least five percent wider than the last.
const BAND_RATIO: f32 = 1.05;

/// Smallest per-band peak, so digital silence cannot divide by zero.
const PEAK_FLOOR: f32 = 3e-3;

/// Per-band peak release per frame, matching NoctaVox. Slow enough that a
/// pause between hits does not reset the band's auto-gain.
const PEAK_RELEASE: f32 = 0.99;

/// Largest per-frame lift applied to a quiet passage, so near-silence cannot
/// blow its noise floor up to the top of the band.
const MAX_FRAME_GAIN: f32 = 64.0;

#[derive(Debug, Clone, Copy)]
struct Band {
    /// Frequency used to place this band on the logarithmic display axis.
    center: f32,
    /// Inclusive FFT bin range.
    start: usize,
    /// Exclusive FFT bin range.
    end: usize,
}

/// Rolling spectrum analyser with the shaping used by NoctaVox.
pub struct Spectrum {
    fft: Fft,
    history: History,
    bands: Vec<Band>,
    /// One normalized magnitude per band for the current frame.
    magnitudes: Vec<f32>,
    /// Running peak per band, used as its auto-gain reference.
    peaks: Vec<f32>,
    /// The decaying bar heights that are drawn.
    bins: Vec<f32>,
    display: Vec<f32>,
    decay: f32,
    sample_rate: u32,
}

impl Spectrum {
    pub fn new(decay: f32) -> Self {
        Self {
            fft: Fft::new(FFT_SIZE),
            history: History::new(FFT_SIZE),
            bands: Vec::new(),
            magnitudes: Vec::new(),
            peaks: Vec::new(),
            bins: Vec::new(),
            display: Vec::new(),
            decay: clamp_decay(decay),
            sample_rate: 0,
        }
    }

    /// Append mono frames, keeping only the newest FFT-sized window.
    pub fn push(&mut self, samples: &[f32]) {
        self.history.push(samples);
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

        if !self.history.is_full() {
            self.release();
            return;
        }

        let output = self.fft.process(self.history.samples());

        // realfft is unnormalized: a full-scale sine peaks near FFT_SIZE / 2.
        let scale = 2.0 / FFT_SIZE as f32;
        for (index, band) in self.bands.iter().enumerate() {
            let values = &output[band.start..band.end];
            let magnitude = if values.is_empty() {
                0.0
            } else {
                values.iter().map(|value| value.norm()).sum::<f32>() / values.len() as f32
            };
            self.magnitudes[index] = magnitude * scale;
        }

        // NoctaVox auto-gain: instant attack, slow release. Each band is
        // measured against its own recent peak, not the frame's, so a quiet
        // part of the mix keeps its column shape.
        let mut loudest = 0.0_f32;
        for (index, peak) in self.peaks.iter_mut().enumerate() {
            *peak = if self.magnitudes[index] > *peak {
                self.magnitudes[index]
            } else {
                (*peak * PEAK_RELEASE).max(PEAK_FLOOR)
            };
            loudest = loudest.max(self.magnitudes[index] / *peak);
        }

        // Without this, a quiet passage measured against peaks from the louder
        // one collapses into a few dots. Lift the whole frame so its loudest
        // band still reaches the top of the band; the gain cap keeps a silent
        // file's noise floor from being blown up to full height.
        let gain = if loudest > 0.0 {
            (1.0 / loudest).min(MAX_FRAME_GAIN)
        } else {
            1.0
        };

        for (index, bin) in self.bins.iter_mut().enumerate() {
            let relative = (self.magnitudes[index] / self.peaks[index] * gain).clamp(0.0, 1.0);
            if relative >= *bin {
                *bin = relative;
            } else {
                *bin *= self.decay;
            }
        }
    }

    /// Place the bands on a logarithmic 20 Hz..20 kHz axis and interpolate them
    /// down to `width` display columns.
    pub fn remap_display(&mut self, width: usize) {
        if width == 0 {
            return;
        }
        if self.bins.is_empty() || self.bands.is_empty() {
            self.display.clear();
            return;
        }

        self.display.resize(width, 0.0);
        if width == 1 {
            let center = (MIN_HZ * MAX_HZ).sqrt();
            self.display[0] = self.value_at(center);
            return;
        }

        let ratio = MAX_HZ / MIN_HZ;
        let denominator = (width - 1) as f32;
        // A column is narrower than a 5 % band above ~800 Hz. Take the tallest
        // band whose center falls inside the column instead of point-sampling,
        // so a narrow high-frequency peak cannot fall between two columns.
        let half_step = ratio.powf(0.5 / denominator);
        for column in 0..width {
            let center = MIN_HZ * ratio.powf(column as f32 / denominator);
            let lo = if column == 0 {
                MIN_HZ
            } else {
                center / half_step
            };
            let hi = if column + 1 == width {
                MAX_HZ
            } else {
                center * half_step
            };
            self.display[column] = self.column_value(lo, hi, center);
        }
    }

    /// The most recent `remap_display` result.
    pub fn display_bins(&self) -> &[f32] {
        &self.display
    }

    /// The tallest band whose center lies in `lo..=hi`, or an interpolation
    /// across the nearest centers when the column is narrower than a band.
    fn column_value(&self, lo: f32, hi: f32, center: f32) -> f32 {
        let start = self.bands.partition_point(|band| band.center < lo);
        let end = self.bands.partition_point(|band| band.center <= hi);
        if start < end {
            self.bins[start..end].iter().copied().fold(0.0, f32::max)
        } else {
            self.value_at(center)
        }
    }

    /// Interpolate the band heights at `frequency` on the log axis.
    fn value_at(&self, frequency: f32) -> f32 {
        let bands = &self.bands;
        if bands.is_empty() {
            return 0.0;
        }
        if frequency <= bands[0].center {
            return self.bins[0];
        }
        let last = bands.len() - 1;
        if frequency >= bands[last].center {
            return self.bins[last];
        }

        let upper = bands.partition_point(|band| band.center < frequency);
        let lower = upper - 1;
        let a = bands[lower].center;
        let b = bands[upper].center;
        let t = ((frequency / a).ln() / (b / a).ln()).clamp(0.0, 1.0);
        self.bins[lower] * (1.0 - t) + self.bins[upper] * t
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
            let end = ((hi / resolution).ceil() as usize).min(FFT_SIZE / 2 + 1);
            bands.push(Band {
                center: (freq + hi) * 0.5,
                start: start.min(end),
                end,
            });
            freq = next;
        }

        let count = bands.len();
        self.bands = bands;
        self.magnitudes.clear();
        self.magnitudes.resize(count, 0.0);
        self.peaks.clear();
        self.peaks.resize(count, PEAK_FLOOR);
        self.bins.clear();
        self.bins.resize(count, 0.0);
        self.display.clear();
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
    use std::f32::consts::TAU;

    fn sine(frequency: f32, sample_rate: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (TAU * frequency * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    fn tone_band(spectrum: &Spectrum, frequency: f32) -> usize {
        spectrum
            .bands
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.center - frequency)
                    .abs()
                    .total_cmp(&(b.center - frequency).abs())
            })
            .map(|(index, _)| index)
            .expect("frequency is covered by a band")
    }

    #[test]
    fn bands_tile_from_20_hz_to_20_khz() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&sine(1_000.0, 44_100, FFT_SIZE));
        spectrum.update(44_100);

        let bands = &spectrum.bands;
        assert!((50..=200).contains(&bands.len()), "{} bands", bands.len());
        assert!(bands[0].center < MIN_HZ * 2.0);
        assert!(bands.last().unwrap().center > MAX_HZ / BAND_RATIO);
        for pair in bands.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "bands must tile by FFT bin");
            assert!(pair[0].center < pair[1].center, "centers must climb");
        }
        for band in bands {
            assert!(band.start < band.end);
            assert!(band.start >= 1 && band.end <= FFT_SIZE / 2 + 1);
        }
    }

    #[test]
    fn a_tone_stays_a_narrow_spike() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&sine(1_000.0, 44_100, FFT_SIZE));
        spectrum.update(44_100);

        let band = tone_band(&spectrum, 1_000.0);
        let loudest = spectrum
            .bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("bins");
        assert!(
            (loudest as i32 - band as i32).abs() <= 1,
            "loudest={loudest} tone={band}"
        );
        assert!(spectrum.bins[band] > 0.9);
        let tall = spectrum.bins.iter().filter(|value| **value > 0.5).count();
        assert!(tall <= 2, "{tall} bands above half height");
        assert!(spectrum.bins[0] < 0.05, "no energy at 20 Hz");
    }

    #[test]
    fn each_band_is_gained_against_its_own_peak() {
        // A 5 kHz component 14 dB below the 1 kHz tone still fills its own
        // band. This per-band auto-gain is what gives the display NoctaVox's
        // wave-like envelope instead of collapsing quiet parts of a mix.
        let sample_rate = 44_100;
        let mixed: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (TAU * 1_000.0 * t).sin() + 0.2 * (TAU * 5_000.0 * t).sin()
            })
            .collect();
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&mixed);
        spectrum.update(sample_rate);

        let loud = tone_band(&spectrum, 1_000.0);
        let quiet = tone_band(&spectrum, 5_000.0);
        assert!(spectrum.bins[loud] > 0.9);
        assert!(spectrum.bins[quiet] > 0.9, "quiet band stayed down");
    }

    #[test]
    fn a_quiet_frame_is_lifted_to_the_full_band() {
        let sample_rate = 44_100;
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&sine(1_000.0, sample_rate, FFT_SIZE));
        spectrum.update(sample_rate);
        let band = tone_band(&spectrum, 1_000.0);
        assert!(spectrum.bins[band] > 0.9);

        let quiet: Vec<f32> = sine(1_000.0, sample_rate, FFT_SIZE)
            .iter()
            .map(|sample| sample * 0.25)
            .collect();
        spectrum.push(&quiet);
        spectrum.update(sample_rate);
        // Against the held peak this frame scores 0.25, but the frame gain
        // lifts its loudest band back to the top of the band.
        assert!(spectrum.bins[band] > 0.9);
    }

    #[test]
    fn the_frame_gain_is_capped_for_near_silence() {
        let sample_rate = 44_100;
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&sine(1_000.0, sample_rate, FFT_SIZE));
        spectrum.update(sample_rate);
        let band = tone_band(&spectrum, 1_000.0);

        let nearly_silent: Vec<f32> = sine(1_000.0, sample_rate, FFT_SIZE)
            .iter()
            .map(|sample| sample * 0.001)
            .collect();
        for _ in 0..40 {
            spectrum.push(&nearly_silent);
            spectrum.update(sample_rate);
        }
        assert!(spectrum.bins[band] < 0.1, "noise floor was amplified");
    }

    #[test]
    fn decays_after_the_tone_stops() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        let sample_rate = 44_100;
        spectrum.push(&sine(1_000.0, sample_rate, FFT_SIZE));
        spectrum.update(sample_rate);
        let band = tone_band(&spectrum, 1_000.0);
        let before = spectrum.bins[band];
        assert!(before > 0.9);

        spectrum.push(&vec![0.0; FFT_SIZE]);
        spectrum.update(sample_rate);
        assert!((spectrum.bins[band] - before * DEFAULT_DECAY).abs() < 1e-5);
    }

    #[test]
    fn one_khz_lands_at_its_log_frequency() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&sine(1_000.0, 44_100, FFT_SIZE));
        spectrum.update(44_100);
        spectrum.remap_display(101);

        let peak = spectrum
            .display
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("display");
        // 20 Hz * 1000^(57/100) is within 2 % of 1 kHz.
        assert!((peak as i32 - 57).abs() <= 1, "peak at column {peak}");

        // Ten kHz is near 90 % of a 20 Hz..20 kHz log axis, not at the peak.
        spectrum.bins.fill(0.0);
        spectrum.push(&sine(10_000.0, 44_100, FFT_SIZE));
        spectrum.update(44_100);
        spectrum.remap_display(101);
        let high = spectrum
            .display
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("display");
        assert!((high as i32 - 90).abs() <= 1, "peak at column {high}");
    }

    #[test]
    fn a_column_uses_the_tallest_band_inside_it() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.bands = [990.0, 1_000.0, 1_010.0]
            .into_iter()
            .map(|center| Band {
                center,
                start: 0,
                end: 0,
            })
            .collect();
        spectrum.bins = vec![0.0, 0.0, 1.0];

        assert_eq!(spectrum.column_value(985.0, 1_015.0, 1_000.0), 1.0);
        assert_eq!(spectrum.column_value(991.0, 999.0, 995.0), 0.0);
    }

    #[test]
    fn interpolates_between_band_centers_on_a_log_axis() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.bands = [100.0, 1_000.0, 10_000.0]
            .into_iter()
            .map(|center| Band {
                center,
                start: 0,
                end: 0,
            })
            .collect();
        spectrum.bins = vec![0.0, 1.0, 0.0];

        assert_eq!(spectrum.value_at(100.0), 0.0);
        assert_eq!(spectrum.value_at(1_000.0), 1.0);
        assert!((spectrum.value_at(316.23) - 0.5).abs() < 1e-4);
        assert!((spectrum.value_at(3_162.3) - 0.5).abs() < 1e-4);
        assert_eq!(spectrum.value_at(10_000.0), 0.0);
        assert_eq!(spectrum.value_at(5.0), 0.0);
        assert_eq!(spectrum.value_at(20_000.0), 0.0);
    }

    #[test]
    fn history_keeps_the_newest_window() {
        let mut spectrum = Spectrum::new(DEFAULT_DECAY);
        spectrum.push(&vec![1.0; FFT_SIZE]);
        spectrum.push(&vec![2.0; 100]);
        let kept = spectrum.history.samples();
        assert_eq!(kept.len(), FFT_SIZE);
        // The oldest 100 samples were dropped; the newest 100 are all 2.0.
        assert_eq!(kept[0], 1.0);
        assert_eq!(kept[FFT_SIZE - 1], 2.0);
        assert_eq!(kept.iter().filter(|sample| **sample == 2.0).count(), 100);

        spectrum.push(&vec![3.0; FFT_SIZE * 2]);
        assert!(
            spectrum
                .history
                .samples()
                .iter()
                .all(|sample| *sample == 3.0)
        );
    }

    #[test]
    fn decay_is_clamped_to_noctavox_range() {
        assert_eq!(Spectrum::new(2.0).decay, 0.97);
        assert_eq!(Spectrum::new(0.0).decay, 0.7);
        assert_eq!(Spectrum::new(f32::NAN).decay, DEFAULT_DECAY);
    }
}
