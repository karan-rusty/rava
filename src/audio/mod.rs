use anyhow::Result;
use std::time::{Duration, Instant};

mod file;
mod gate;
mod live;
pub mod synth;

pub(crate) use file::FileSource;
pub use gate::SilenceGate;
pub use live::LiveSource;
pub use synth::TestSource;

pub const CHUNK_FRAMES: usize = 735;

pub trait Capture {
    /// Replaces `out` with the next chunk of interleaved stereo frames.
    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize>;
    fn sample_rate(&self) -> u32;
}

pub struct Pacer {
    start: Instant,
    frames: u64,
    rate: u32,
}

impl Pacer {
    pub fn new(rate: u32) -> Self {
        Self {
            start: Instant::now(),
            frames: 0,
            rate,
        }
    }

    pub fn wait(&mut self, frames: usize) {
        self.frames += frames as u64;
        let due = Duration::from_secs_f64(self.frames as f64 / self.rate as f64);
        if let Some(rest) = due.checked_sub(self.start.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}
