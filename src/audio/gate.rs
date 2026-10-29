//! Silence detection for live capture.
//!
//! The visualizer starts blank, wakes on the first audible chunk, and blanks
//! again only after the input has been quiet for a while, so gaps between
//! songs do not make it flicker.

use std::time::{Duration, Instant};

pub const DEFAULT_THRESHOLD_DB: f32 = -60.0;

pub struct SilenceGate {
    /// Linear RMS threshold derived from dBFS.
    threshold: f32,
    idle_after: Duration,
    quiet_since: Option<Instant>,
    active: bool,
}

impl SilenceGate {
    pub fn new(threshold_db: f32, idle_after: Duration) -> Self {
        let db = if threshold_db.is_finite() {
            threshold_db.clamp(-120.0, -1.0)
        } else {
            DEFAULT_THRESHOLD_DB
        };
        Self {
            threshold: 10f32.powf(db / 20.0),
            idle_after,
            quiet_since: None,
            active: false,
        }
    }

    /// Feed one interleaved PCM chunk. Returns whether the screen should be
    /// drawn right now; a return value of `false` means "stay off".
    pub fn observe(&mut self, samples: &[f32], now: Instant) -> bool {
        if rms(samples) >= self.threshold {
            self.active = true;
            self.quiet_since = None;
        } else if self.active {
            let quiet_since = *self.quiet_since.get_or_insert(now);
            if now.saturating_duration_since(quiet_since) >= self.idle_after {
                self.active = false;
                self.quiet_since = None;
            }
        }
        self.active
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.active
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    (sum / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK_MS: u64 = 17;

    fn chunk(amplitude: f32) -> Vec<f32> {
        // Interleaved stereo, like every source produces.
        vec![amplitude; 735 * 2]
    }

    #[test]
    fn starts_off_until_the_first_sound() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD_DB, Duration::from_millis(500));
        let now = Instant::now();
        assert!(!gate.observe(&chunk(0.0), now));
        assert!(!gate.is_active());
    }

    #[test]
    fn wakes_immediately_on_sound() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD_DB, Duration::from_millis(500));
        let now = Instant::now();
        gate.observe(&chunk(0.0), now);
        assert!(gate.observe(&chunk(0.1), now + Duration::from_millis(CHUNK_MS)));
    }

    #[test]
    fn stays_on_during_a_short_gap() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD_DB, Duration::from_millis(500));
        let start = Instant::now();
        assert!(gate.observe(&chunk(0.1), start));
        assert!(gate.observe(&chunk(0.0), start + Duration::from_millis(CHUNK_MS * 10)));
        assert!(gate.observe(&chunk(0.1), start + Duration::from_millis(CHUNK_MS * 20)));
    }

    #[test]
    fn blanks_after_the_grace_period() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD_DB, Duration::from_millis(200));
        let start = Instant::now();
        assert!(gate.observe(&chunk(0.1), start));
        assert!(gate.observe(&chunk(0.0), start + Duration::from_millis(100)));
        assert!(!gate.observe(&chunk(0.0), start + Duration::from_millis(350)));
        assert!(!gate.is_active());
    }

    #[test]
    fn quiet_audio_below_the_threshold_counts_as_silence() {
        let mut gate = SilenceGate::new(-40.0, Duration::ZERO);
        assert!(gate.observe(&chunk(0.1), Instant::now()));
        assert!(!gate.observe(&chunk(0.0001), Instant::now()));
    }

    #[test]
    fn a_new_sound_wakes_it_up_again() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD_DB, Duration::ZERO);
        let start = Instant::now();
        assert!(gate.observe(&chunk(0.1), start));
        assert!(!gate.observe(&chunk(0.0), start));
        assert!(gate.observe(&chunk(0.1), start + Duration::from_millis(50)));
    }

    #[test]
    fn out_of_phase_stereo_does_not_look_silent() {
        let mut gate = SilenceGate::new(DEFAULT_THRESHOLD_DB, Duration::ZERO);
        let mut samples = Vec::with_capacity(735 * 2);
        for _ in 0..735 {
            samples.push(0.5);
            samples.push(-0.5);
        }
        assert!(gate.observe(&samples, Instant::now()));
    }

    #[test]
    fn non_finite_thresholds_fall_back_to_the_default() {
        let gate = SilenceGate::new(f32::NAN, Duration::ZERO);
        assert_eq!(gate.threshold, 10f32.powf(DEFAULT_THRESHOLD_DB / 20.0));
    }
}
