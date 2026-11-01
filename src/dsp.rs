//! FFT plumbing shared by the bars and the spectrum.

use std::f32::consts::TAU;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// A planned real FFT with a Hann window and reusable buffers.
pub(crate) struct Fft {
    plan: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl Fft {
    pub(crate) fn new(size: usize) -> Self {
        let plan = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        let window = (0..size)
            .map(|i| 0.5 - 0.5 * (TAU * i as f32 / size as f32).cos())
            .collect();
        Self {
            input: plan.make_input_vec(),
            output: plan.make_output_vec(),
            scratch: plan.make_scratch_vec(),
            plan,
            window,
        }
    }

    /// Window and transform exactly one buffer of samples.
    pub(crate) fn process(&mut self, samples: &[f32]) -> &[Complex<f32>] {
        debug_assert_eq!(samples.len(), self.input.len());
        for (input, (sample, window)) in self
            .input
            .iter_mut()
            .zip(samples.iter().zip(self.window.iter()))
        {
            *input = sample * window;
        }
        self.plan
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .expect("FFT buffer sizes are fixed");
        &self.output
    }
}

/// A fixed-size sliding window over the newest samples, zero-padded until full.
pub(crate) struct History {
    samples: Vec<f32>,
    filled: usize,
}

impl History {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            samples: vec![0.0; size],
            filled: 0,
        }
    }

    /// Append samples, dropping the oldest so only `size` remain.
    pub(crate) fn push(&mut self, samples: &[f32]) {
        let size = self.samples.len();
        if samples.len() >= size {
            self.samples
                .copy_from_slice(&samples[samples.len() - size..]);
            self.filled = size;
            return;
        }
        self.samples.copy_within(samples.len().., 0);
        self.samples[size - samples.len()..].copy_from_slice(samples);
        self.filled = (self.filled + samples.len()).min(size);
    }

    pub(crate) fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub(crate) fn is_full(&self) -> bool {
        self.filled == self.samples.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_only_the_newest_samples() {
        let mut history = History::new(4);
        assert!(!history.is_full());
        history.push(&[1.0, 2.0]);
        assert!(!history.is_full());
        history.push(&[3.0, 4.0]);
        assert!(history.is_full());
        assert_eq!(history.samples(), &[1.0, 2.0, 3.0, 4.0]);

        history.push(&[5.0]);
        assert_eq!(history.samples(), &[2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn oversized_push_keeps_the_tail_and_fills_the_window() {
        let mut history = History::new(3);
        history.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!(history.is_full());
        assert_eq!(history.samples(), &[3.0, 4.0, 5.0]);
    }
}
