use anyhow::Result;

use super::{CHUNK_FRAMES, Capture, Pacer};

pub struct TestSource {
    rate: u32,
    position: u64,
    noise_state: u64,
    last_noise: f32,
    pacer: Pacer,
}

impl TestSource {
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(8_000),
            position: 0,
            noise_state: 0x9e37_79b9_7f4a_7c15,
            last_noise: 0.0,
            pacer: Pacer::new(rate.max(8_000)),
        }
    }

    fn noise(&mut self) -> f32 {
        self.noise_state = self
            .noise_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(144_269_504_088_963_407);
        let value = (self.noise_state >> 33) as f32 / 2_147_483_647.0;
        value * 2.0 - 1.0
    }

    fn sample(&mut self, t: f64) -> (f32, f32) {
        const BEAT: f64 = 0.5;
        let tau = std::f64::consts::TAU;

        let roots = [55.0, 43.64, 65.41, 49.0];
        let chords = [
            [220.0, 261.63, 329.63],
            [174.61, 220.0, 261.63],
            [261.63, 329.63, 392.0],
            [196.0, 246.94, 293.66],
        ];
        let chord_index = ((t / 2.0) as usize) % 4;
        let chord = chords[chord_index];

        let beat_phase = t % BEAT;
        let kick_envelope = (-beat_phase * 9.0).exp();
        let kick_phase =
            tau * (52.0 * beat_phase + 90.0 * (1.0 - (-beat_phase * 28.0).exp()) / 28.0);
        let kick = kick_phase.sin() * kick_envelope * 0.55;

        let bass_envelope = (-beat_phase * 9.0).exp();
        let bass_frequency = roots[chord_index];
        let bass = (tau * bass_frequency * t).sin() * bass_envelope
            + 0.25 * (tau * bass_frequency * 2.0 * t).sin() * bass_envelope;
        let pad_level = 0.045 * (0.7 + 0.3 * (t * 0.55).sin());
        let mut pad_left = 0.0;
        let mut pad_right = 0.0;
        for (index, frequency) in chord.iter().enumerate() {
            let phase = index as f64 * 0.7;
            pad_left += (tau * frequency * t + phase).sin() * pad_level;
            pad_right += (tau * frequency * 1.003 * t + phase).sin() * pad_level;
        }

        let step = (t / 0.25) as usize;
        let note = chord[step % 3] * 2.0;
        let local = t % 0.25;
        let arp_envelope = (-local * 5.5).exp() * 0.16;
        let arp = (tau * note * t).sin()
            + 0.35 * (tau * note * 2.0 * t + 0.2).sin()
            + 0.12 * (tau * note * 3.0 * t).sin();
        let arp = arp * arp_envelope;
        let pan: f64 = if step.is_multiple_of(2) { -0.35 } else { 0.35 };
        let left_gain = ((1.0 - pan) * 0.5).sqrt();
        let right_gain = ((1.0 + pan) * 0.5).sqrt();

        let noise = self.noise();
        let high_noise = (noise - self.last_noise) as f64;
        self.last_noise = noise;

        let hat_phase = t % 0.125;
        let hat = high_noise * (-hat_phase * 70.0).exp() * 0.12;
        let hat = if step.is_multiple_of(2) { -hat } else { hat };

        let snare_phase = (t + 0.5) % 1.0;
        let snare = noise as f64 * (-snare_phase * 32.0).exp() * 0.06;

        let air =
            (high_noise * 0.55 + noise as f64 * 1.2) * 0.024 * (0.75 + 0.25 * (t * 1.3).sin());
        let mut left = kick + bass + pad_left + arp * left_gain + hat * 0.8 + snare + air;
        let mut right = kick + bass + pad_right + arp * right_gain - hat * 0.8 + snare + air;

        left = (left * 1.1).tanh() * 0.85;
        right = (right * 1.1).tanh() * 0.85;
        (left as f32, right as f32)
    }
}

impl Capture for TestSource {
    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        self.pacer.wait(CHUNK_FRAMES);
        let rate = self.rate as f64;
        out.clear();
        out.reserve(CHUNK_FRAMES * 2);
        for frame in 0..CHUNK_FRAMES {
            let t = (self.position + frame as u64) as f64 / rate;
            let (left, right) = self.sample(t);
            out.push(left);
            out.push(right);
        }
        self.position += CHUNK_FRAMES as u64;
        Ok(CHUNK_FRAMES)
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }
}
