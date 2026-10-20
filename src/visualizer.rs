//! Ported NoctaVox visualizer data paths: the playback sample tap, log-spaced
//! FFT bands with per-band auto-gain, and whole-file waveform buckets.

use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use anyhow::{Result, anyhow};
use spectrum_analyzer::{FrequencyLimit, samples_fft_to_spectrum, windows::hann_window};
use voxio::{BinMetric, TapHandle, Waveform, WaveformOptions};

pub const TAP_BUFFER_CAPACITY: usize = 2048;
pub const WF_BIN_LEN: usize = 500;
pub const OSCILLO_LIMITER: usize = 512;

const PEAK_FLOOR: f32 = 3e-3;
const SMOOTHNESS_STEP: f32 = 0.5;
const SMOOTHNESS_MIN: f32 = 0.1;
const SMOOTHNESS_MAX: f32 = 3.9;

const SPECTRUM_MIN_HZ: f32 = 20.0;
const SPECTRUM_MAX_HZ: f32 = 20_000.0;
const BAND_RATIO: f32 = 1.05;

/// The visualizers NoctaVox cycles through with `w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ProgressDisplay {
    // name = matches NoctaVox's config spelling of this display
    #[value(name = "progress_bar")]
    ProgressBar,
    Waveform,
    Oscilloscope,
    #[default]
    Spectrum,
}

impl ProgressDisplay {
    pub fn next(self) -> Self {
        match self {
            Self::ProgressBar => Self::Waveform,
            Self::Waveform => Self::Oscilloscope,
            Self::Oscilloscope => Self::Spectrum,
            Self::Spectrum => Self::ProgressBar,
        }
    }
}

impl std::fmt::Display for ProgressDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProgressBar => write!(f, "progress bar"),
            Self::Waveform => write!(f, "waveform"),
            Self::Oscilloscope => write!(f, "oscilloscope"),
            Self::Spectrum => write!(f, "spectrum"),
        }
    }
}

pub struct Visualizer {
    tap: TapHandle,
    display_tap: VecDeque<f32>,
    spectrum: SpectrumState,
    waveform: WaveformManager,
    mode: ProgressDisplay,
}

impl Visualizer {
    pub fn new(tap: TapHandle, spectrum_decay: f32) -> Self {
        Visualizer {
            tap,
            display_tap: VecDeque::with_capacity(TAP_BUFFER_CAPACITY),
            spectrum: SpectrumState::new(spectrum_decay),
            waveform: WaveformManager::new(),
            mode: ProgressDisplay::default(),
        }
    }

    pub fn mode(&self) -> ProgressDisplay {
        self.mode
    }

    pub fn set_mode(&mut self, mode: ProgressDisplay) {
        self.mode = mode;
    }

    pub fn next_mode(&mut self) {
        self.mode = self.mode.next();
    }

    /// Pull whatever the audio thread wrote since the last frame, folded to mono.
    pub fn fill_tap(&mut self, channels: usize) {
        let channels = channels.max(1);
        let latest = self.tap.latest(TAP_BUFFER_CAPACITY * channels);
        for frame in latest.chunks_exact(channels) {
            let mono = frame.iter().copied().sum::<f32>() / channels as f32;
            self.display_tap.push_back(mono);
        }

        let overflow = self.display_tap.len().saturating_sub(TAP_BUFFER_CAPACITY);
        self.display_tap.drain(..overflow);
    }

    /// Drop stale samples: they belong to the position we just left.
    pub fn flush_tap(&mut self) {
        self.display_tap.clear();
        self.tap.latest(usize::MAX);
        self.spectrum.reset();
    }

    pub fn display_tap(&mut self) -> &[f32] {
        self.display_tap.make_contiguous()
    }

    pub fn update_spectrum(&mut self, sample_rate: u32) {
        if !self.display_tap.is_empty() {
            let samples = self.display_tap.make_contiguous();
            self.spectrum.update(samples, sample_rate);
        }
    }

    /// Reservoir-sample the log-spaced bands down to the canvas width.
    pub fn spectrum_bins(&mut self, width: usize) -> &[f32] {
        self.spectrum.remap_display(width);
        self.spectrum.display_bins()
    }

    pub fn request_waveform(&mut self, path: &str) {
        self.waveform.request(path);
    }

    /// Pick up the decoded waveform once its worker thread is done.
    pub fn poll_waveform(&mut self) {
        if let Some(result) = self.waveform.poll() {
            self.waveform.complete(result);
        }
    }

    pub fn waveform_is_valid(&self) -> bool {
        self.waveform.bins.is_some()
    }

    pub fn waveform(&self) -> &[f32] {
        &self.waveform.smoothed_view
    }

    pub fn smoothing(&self) -> f32 {
        self.waveform.smoothing_factor
    }

    pub fn increment_smoothness(&mut self, up: bool) {
        self.waveform.increment_smoothness(up);
    }
}

/// FFT bars: one band per ~5% frequency step from 20 Hz to 20 kHz, each
/// normalized against its own running peak so quiet and loud songs both fill
/// the screen. Instant attack, exponential release.
pub struct SpectrumState {
    bins: Vec<f32>,
    display_bins: Vec<f32>,
    decay_factor: f32,
    bands: Vec<(f32, f32)>,
    band_peaks: Vec<f32>,
    sample_rate: u32,
    last_display_width: usize,
    bins_dirty: bool,
}

impl SpectrumState {
    pub fn new(decay_factor: f32) -> Self {
        SpectrumState {
            bins: Vec::new(),
            display_bins: Vec::new(),
            band_peaks: Vec::new(),
            bands: Vec::new(),
            decay_factor,
            sample_rate: 0,
            last_display_width: 0,
            bins_dirty: false,
        }
    }

    pub fn update(&mut self, samples: &[f32], sample_rate: u32) {
        if sample_rate == 0 {
            return;
        }

        let fft_size = TAP_BUFFER_CAPACITY;

        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            let freq_resolution = sample_rate as f32 / fft_size as f32;
            self.bands.clear();
            let mut freq = SPECTRUM_MIN_HZ;
            while freq < SPECTRUM_MAX_HZ {
                let next = (freq * BAND_RATIO).max(freq + freq_resolution);
                self.bands.push((freq, next.min(SPECTRUM_MAX_HZ)));
                freq = next;
            }
            let n = self.bands.len();
            self.band_peaks.resize(n, PEAK_FLOOR);
            self.bins.resize(n, 0.0);
        }

        if samples.len() < fft_size {
            for bin in self.bins.iter_mut() {
                *bin *= self.decay_factor;
            }
            return;
        }

        let start = samples.len() - fft_size;
        let windowed = hann_window(&samples[start..]);

        let spectrum = match samples_fft_to_spectrum(
            &windowed,
            self.sample_rate,
            FrequencyLimit::Range(SPECTRUM_MIN_HZ, SPECTRUM_MAX_HZ),
            None,
        ) {
            Ok(s) => s,
            Err(_) => {
                for bin in self.bins.iter_mut() {
                    *bin *= self.decay_factor;
                }
                return;
            }
        };

        let mut data_iter = spectrum.data().iter().peekable();

        for i in 0..self.bands.len() {
            let (lo, hi) = self.bands[i];
            let mut sum = 0.0_f32;
            let mut count = 0_usize;

            while let Some(&(f, m)) = data_iter.peek() {
                let freq_val = f.val();
                if freq_val < lo {
                    data_iter.next();
                } else if freq_val < hi {
                    sum += m.val();
                    count += 1;
                    data_iter.next();
                } else {
                    break;
                }
            }

            let mag = if count > 0 { sum / count as f32 } else { 0.0 };
            let normalized = mag / (fft_size as f32 / 2.0);

            // Per-band auto-gain: instant attack, slow release
            if normalized > self.band_peaks[i] {
                self.band_peaks[i] = normalized;
            } else {
                self.band_peaks[i] = (self.band_peaks[i] * 0.99).max(1e-3);
            }

            let relative = (normalized / self.band_peaks[i]).clamp(0.0, 1.0);

            if relative > self.bins[i] {
                self.bins[i] = relative;
            } else {
                self.bins[i] *= self.decay_factor;
            }
        }

        self.bins_dirty = true;
    }

    pub fn remap_display(&mut self, width: usize) {
        if width == 0
            || self.bins.is_empty()
            || (!self.bins_dirty && self.last_display_width == width)
        {
            return;
        }

        let num_bins = self.bins.len();
        self.display_bins.resize(width, 0.0);
        self.display_bins
            .iter_mut()
            .enumerate()
            .for_each(|(i, out)| {
                let t = i as f32 / (width - 1).max(1) as f32;
                let src = t * (num_bins - 1) as f32;
                let lo = src.floor() as usize;
                let hi = (lo + 1).min(num_bins - 1);
                let frac = src - lo as f32;
                *out = self.bins[lo] * (1.0 - frac) + self.bins[hi] * frac;
            });
        self.last_display_width = width;
        self.bins_dirty = false;
    }

    pub fn display_bins(&self) -> &[f32] {
        &self.display_bins
    }

    pub fn reset(&mut self) {
        let n = self.bands.len();
        self.band_peaks.clear();
        self.band_peaks.resize(n, PEAK_FLOOR);
        self.bins.clear();
        self.bins.resize(n, 0.0);
        self.display_bins.fill(0.0);
        self.bins_dirty = true;
    }
}

/// Whole-file amplitude buckets, decoded once per track on a worker thread.
pub struct WaveformManager {
    bins: Option<Vec<f32>>,
    smoothed_view: Vec<f32>,
    smoothing_factor: f32,
    receiver: Option<Receiver<Result<Waveform>>>,
}

impl WaveformManager {
    pub fn new() -> Self {
        WaveformManager {
            bins: None,
            smoothed_view: Vec::with_capacity(WF_BIN_LEN),
            smoothing_factor: 1.0,
            receiver: None,
        }
    }

    pub fn request(&mut self, path: &str) {
        // shortcut: no cache — the file is decoded on every launch. Add a
        // waveform cache if start-up on long tracks starts to hurt.
        let path = path.to_owned();
        let (tx, rx) = mpsc::sync_channel(1);

        thread::spawn(move || {
            let result = Waveform::generate(
                &path,
                &WaveformOptions {
                    bins: WF_BIN_LEN,
                    metric: BinMetric::Rms,
                    highpass_hz: Some(350.0),
                    treble_db: 9.0,
                },
            )
            .map(|mut wf| {
                wf.local_normalize(0.8, 20);
                wf.normalize();
                wf.contrast(1.5);
                wf
            })
            .map_err(|e| anyhow!("{e}"));

            let _ = tx.send(result);
        });

        self.receiver = Some(rx);
    }

    pub fn poll(&mut self) -> Option<Result<Waveform>> {
        match self.receiver.as_ref()?.try_recv() {
            Ok(result) => {
                self.receiver = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                None
            }
        }
    }

    pub fn complete(&mut self, result: Result<Waveform>) {
        match result {
            Ok(waveform) => {
                self.bins = Some(waveform.bins);
                self.apply_smoothing();
            }
            Err(_) => self.bins = None,
        }
    }

    fn apply_smoothing(&mut self) {
        if let Some(bins) = &self.bins {
            self.smoothed_view = smooth_waveform(bins, self.smoothing_factor);
        }
    }

    fn increment_smoothness(&mut self, up: bool) {
        let next = if up {
            self.smoothing_factor + SMOOTHNESS_STEP
        } else {
            self.smoothing_factor - SMOOTHNESS_STEP
        };
        let clamped = next.clamp(SMOOTHNESS_MIN, SMOOTHNESS_MAX);
        if clamped != self.smoothing_factor {
            self.smoothing_factor = clamped;
            self.apply_smoothing();
        }
    }
}

/// Weighted moving average over the waveform, weight falling off past the
/// (fractional) smoothing factor.
fn smooth_waveform(waveform: &[f32], smoothing_factor: f32) -> Vec<f32> {
    if waveform.len() <= (smoothing_factor.ceil() as usize * 2 + 1) {
        return waveform.to_vec();
    }

    let range = smoothing_factor.ceil() as isize;

    waveform
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let mut sum = 0.0;
            let mut total_weight = 0.0;

            for offset in -range..=range {
                let idx = i as isize + offset;
                if idx >= 0 && idx < waveform.len() as isize {
                    let distance = offset.abs() as f32;
                    let weight = if distance <= smoothing_factor {
                        1.0
                    } else {
                        1.0 - (distance - smoothing_factor)
                    };

                    if weight > 0.0 {
                        sum += waveform[idx as usize] * weight;
                        total_weight += weight;
                    }
                }
            }

            if total_weight > 0.0 {
                sum / total_weight
            } else {
                waveform[i]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sample_rate: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn spectrum_bands_are_log_spaced() {
        let mut state = SpectrumState::new(0.85);
        state.update(&sine(1000.0, 44_100, TAP_BUFFER_CAPACITY), 44_100);

        assert!(
            (50..=200).contains(&state.bands.len()),
            "{} bands",
            state.bands.len()
        );
        assert_eq!(state.bands[0].0, SPECTRUM_MIN_HZ);
        for (lo, hi) in &state.bands {
            assert!(hi > lo && *hi <= SPECTRUM_MAX_HZ, "band {lo}..{hi}");
        }
        for pair in state.bands.windows(2) {
            assert!(pair[1].0 >= pair[0].1, "bands must tile upward");
        }
    }

    #[test]
    fn spectrum_finds_the_tone_and_decays_after_it_stops() {
        let mut state = SpectrumState::new(0.85);
        let sample_rate = 44_100;
        state.update(&sine(1000.0, sample_rate, TAP_BUFFER_CAPACITY), sample_rate);

        let tone_band = state
            .bands
            .iter()
            .position(|(lo, hi)| *lo <= 1000.0 && 1000.0 < *hi)
            .expect("1 kHz band");
        let loudest = state
            .bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .expect("bins");
        // The loudest bar sits on the tone (its neighbour may tie on leakage).
        assert!(
            (loudest as i32 - tone_band as i32).abs() <= 1,
            "loudest={loudest} tone={tone_band}"
        );
        assert!(state.bins[tone_band] > 0.1);
        assert!(state.bins[0] < 0.05, "no energy at 20 Hz");

        state.remap_display(64);
        assert_eq!(state.display_bins.len(), 64);
        let peak_idx = state
            .display_bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .expect("display bins");
        let (lo, hi) =
            state.bands[(peak_idx as f32 / 63.0 * (state.bands.len() - 1) as f32).round() as usize];
        assert!(lo <= 2000.0 && hi >= 500.0, "peak sits at {lo}..{hi} Hz");

        // Silence: bars decay by the theme factor instead of snapping to zero.
        let before = state.bins[tone_band];
        state.update(&vec![0.0; TAP_BUFFER_CAPACITY], sample_rate);
        assert!((state.bins[tone_band] - before * 0.85).abs() < 1e-6);
    }

    #[test]
    fn smoothing_averages_neighbours_and_keeps_length() {
        let wf: Vec<f32> = (0..64).map(|i| i as f32).collect();
        let smoothed = smooth_waveform(&wf, 1.0);
        assert_eq!(smoothed.len(), wf.len());
        // A ramp's weighted average stays between its ends and only flattens.
        assert!((smoothed[32] - 32.0).abs() < 0.5);
        assert!(smoothed[0] >= wf[0] && smoothed[0] < wf[2]);
        assert!(smoothed.iter().copied().fold(f32::MIN, f32::max) <= 63.0);

        let short = smooth_waveform(&[1.0, 2.0, 3.0], 3.0);
        assert_eq!(short, vec![1.0, 2.0, 3.0]);
    }
}
