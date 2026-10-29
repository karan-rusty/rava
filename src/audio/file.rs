use std::path::Path;

use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavReader};

use super::{CHUNK_FRAMES, Capture, Pacer};

pub struct FileSource {
    samples: Vec<i16>,
    frames: usize,
    position: usize,
    looping: bool,
    rate: u32,
    pacer: Pacer,
}

impl FileSource {
    pub fn open(path: &str, target_rate: u32, looping: bool) -> Result<Self> {
        let path_ref = Path::new(path);
        let mut reader = WavReader::open(path_ref)
            .with_context(|| format!("could not read WAV file `{path}`"))?;
        let spec = reader.spec();
        if spec.channels == 0 {
            bail!("WAV file has no channels");
        }
        let source_rate = spec.sample_rate.max(1);
        let channels = spec.channels as usize;

        let raw: Vec<f32> = match spec.sample_format {
            SampleFormat::Float => {
                let mut values = Vec::new();
                for sample in reader.samples::<f32>() {
                    values.push(sample.context("invalid float sample in WAV file")?);
                }
                values
            }
            SampleFormat::Int => {
                let bits = spec.bits_per_sample.clamp(1, 32);
                let max = (1u64 << (bits - 1)) as f32;
                let mut values = Vec::new();
                for sample in reader.samples::<i32>() {
                    let value = sample.context("invalid integer sample in WAV file")?;
                    values.push(value as f32 / max);
                }
                values
            }
        };

        let source_frames = raw.len() / channels;
        if source_frames == 0 {
            bail!("WAV file contains no audio");
        }
        let target_frames =
            ((source_frames as f64) * target_rate as f64 / source_rate as f64).round() as usize;
        let mut samples = Vec::with_capacity(target_frames * 2);
        for frame in 0..target_frames {
            let source_position = frame as f64 * source_rate as f64 / target_rate as f64;
            let index = source_position.floor() as usize;
            let fraction = (source_position - index as f64) as f32;
            let next = (index + 1).min(source_frames - 1);
            for channel in 0..2 {
                let source_channel = channel.min(channels - 1);
                let a = raw[index * channels + source_channel];
                let b = raw[next * channels + source_channel];
                let value = a + (b - a) * fraction;
                samples.push((value.clamp(-1.0, 1.0) * 32_767.0).round() as i16);
            }
        }
        Ok(Self {
            frames: target_frames,
            samples,
            position: 0,
            looping,
            rate: target_rate,
            pacer: Pacer::new(target_rate),
        })
    }
}

impl Capture for FileSource {
    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        self.pacer.wait(CHUNK_FRAMES);
        if self.position >= self.frames {
            if self.looping {
                self.position = 0;
            } else {
                bail!("end of WAV file");
            }
        }
        let frames = (self.frames - self.position).min(CHUNK_FRAMES);
        out.clear();
        out.reserve(frames * 2);
        let start = self.position * 2;
        let end = start + frames * 2;
        for sample in &self.samples[start..end] {
            out.push(*sample as f32 / 32_768.0);
        }
        self.position += frames;
        Ok(frames)
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
}
