use realfft::RealFftPlanner;
use realfft::RealToComplex;
use std::sync::Arc;

const FFT_SIZE: usize = 2048;
const BARS: usize = 32;
const FLOOR_DB: f32 = -70.0;

pub struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<realfft::num_complex::Complex<f32>>,
    history: Vec<f32>,
    bands: Vec<(usize, usize)>,
    values: Vec<f32>,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let input = fft.make_input_vec();
        let spectrum = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        let bands = make_bands(sample_rate);
        Self {
            fft,
            window,
            input,
            spectrum,
            scratch,
            history: vec![0.0; FFT_SIZE],
            bands,
            values: vec![0.0; BARS],
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        if samples.len() >= FFT_SIZE {
            self.history
                .copy_from_slice(&samples[samples.len() - FFT_SIZE..]);
            return;
        }
        let keep = FFT_SIZE - samples.len();
        self.history.copy_within(samples.len().., 0);
        self.history[keep..].copy_from_slice(samples);
    }

    pub fn update(&mut self, dt: f32) {
        for (i, sample) in self.history.iter().enumerate() {
            self.input[i] = sample * self.window[i];
        }
        self.fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .expect("FFT buffer sizes are fixed");

        let scale = 2.0 / FFT_SIZE as f32;
        for (i, (start, end)) in self.bands.iter().enumerate() {
            let end = (*end).min(self.spectrum.len());
            let start = (*start).min(end.saturating_sub(1));
            let mean = self.spectrum[start..end]
                .iter()
                .map(|value| (value.re * value.re + value.im * value.im).sqrt())
                .sum::<f32>()
                / (end - start).max(1) as f32;
            let db = 20.0 * (mean * scale + 1e-9).log10();
            let target = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
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
