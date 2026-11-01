use std::path::Path;

use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavReader};

use super::{CHUNK_FRAMES, Capture, Pacer};

pub struct FileSource {
    samples: Vec<f32>,
    frames: usize,
    position: usize,
    rate: u32,
    pacer: Pacer,
}

impl FileSource {
    pub fn open(path: &str, target_rate: u32) -> Result<Self> {
        let (raw, channels, source_rate) = decode(path)?;
        let source_frames = raw.len() / channels;
        if source_frames == 0 {
            bail!("WAV file contains no audio");
        }

        let target_rate = target_rate.max(1);
        let samples = resample(&raw, channels, source_rate, target_rate);
        let frames = samples.len() / 2;
        Ok(Self {
            samples,
            frames,
            position: 0,
            rate: target_rate,
            pacer: Pacer::new(target_rate),
        })
    }
}

impl Capture for FileSource {
    fn read(&mut self, out: &mut Vec<f32>) -> Result<()> {
        self.pacer.wait(CHUNK_FRAMES);
        if self.position >= self.frames {
            self.position = 0;
        }
        let frames = (self.frames - self.position).min(CHUNK_FRAMES);
        let start = self.position * 2;
        let end = start + frames * 2;
        out.clear();
        out.extend_from_slice(&self.samples[start..end]);
        self.position += frames;
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }
}

/// Decode a WAV into interleaved `f32`, returning channels and source rate.
fn decode(path: &str) -> Result<(Vec<f32>, usize, u32)> {
    let path_ref = Path::new(path);
    let mut reader =
        WavReader::open(path_ref).with_context(|| format!("could not read WAV file `{path}`"))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        bail!("WAV file has no channels");
    }
    let channels = spec.channels as usize;

    let raw = match spec.sample_format {
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

    Ok((raw, channels, spec.sample_rate.max(1)))
}

/// Resample interleaved audio to stereo at `target_rate`.
///
/// Downsampling averages every source sample that contributes to an output
/// frame, which low-passes the stream and keeps content above the new Nyquist
/// frequency from folding back as an audible alias. Upsampling interpolates
/// linearly. The averaging width equals one output period, so the cost is
/// proportional to the input length rather than the ratio.
///
/// shortcut: a fractional box filter is only ~4 dB down at the new Nyquist;
/// upgrade to a polyphase windowed-sinc filter if alias rejection matters.
fn resample(raw: &[f32], channels: usize, source_rate: u32, target_rate: u32) -> Vec<f32> {
    let channels = channels.max(1);
    let source_frames = raw.len() / channels;
    if source_frames == 0 {
        return Vec::new();
    }
    let step = source_rate as f64 / target_rate as f64;
    let target_frames = ((source_frames as f64 / step).round() as usize).max(1);
    let mut out = Vec::with_capacity(target_frames * 2);

    for frame in 0..target_frames {
        let position = frame as f64 * step;
        for channel in 0..2 {
            let source_channel = channel.min(channels - 1);
            let value = if step > 1.0 {
                average_window(raw, channels, source_channel, position, step)
            } else {
                let index = position.floor() as usize;
                let fraction = (position - index as f64) as f32;
                let a = raw[index * channels + source_channel];
                let next = (index + 1).min(source_frames.saturating_sub(1));
                let b = raw[next * channels + source_channel];
                a + (b - a) * fraction
            };
            out.push(if value.is_finite() {
                value.clamp(-1.0, 1.0)
            } else {
                0.0
            });
        }
    }
    out
}

/// Fractional box average over the source range covered by one output frame.
fn average_window(raw: &[f32], channels: usize, channel: usize, start: f64, width: f64) -> f32 {
    let frames = raw.len() / channels;
    let end = (start + width).min(frames as f64);
    let first = start.floor() as usize;
    let last = (end.ceil() as usize).min(frames);
    let mut sum = 0.0;
    let mut total = 0.0;
    for frame in first..last {
        let begin = (frame as f64).max(start);
        let finish = ((frame + 1) as f64).min(end);
        if finish <= begin {
            continue;
        }
        let weight = (finish - begin) as f32;
        sum += raw[frame * channels + channel] * weight;
        total += weight;
    }
    if total > 0.0 {
        sum / total
    } else {
        raw[first.min(frames - 1) * channels + channel]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn sine(frequency: f32, rate: u32, frames: usize, channels: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * channels);
        for frame in 0..frames {
            let value = (TAU * frequency * frame as f32 / rate as f32).sin();
            for _ in 0..channels {
                out.push(value);
            }
        }
        out
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn downsampling_rejects_content_above_the_new_nyquist() {
        let raw = sine(36_000.0, 96_000, 48_000, 2);
        let out = resample(&raw, 2, 96_000, 44_100);
        let before = rms(&raw);
        let after = rms(&out);
        assert!(
            after < before * 0.3,
            "36 kHz should be filtered, got {after:.3} vs {before:.3}"
        );
    }

    #[test]
    fn downsampling_keeps_audible_tones() {
        let raw = sine(1_000.0, 96_000, 48_000, 2);
        let out = resample(&raw, 2, 96_000, 44_100);
        assert!((rms(&out) / rms(&raw) - 1.0).abs() < 0.05);
    }

    #[test]
    fn upsampling_produces_the_requested_length() {
        let raw = sine(1_000.0, 8_000, 400, 1);
        let out = resample(&raw, 1, 8_000, 44_100);
        assert_eq!(out.len(), 2205 * 2);
        // Mono is duplicated to both stereo channels.
        for frame in out.as_chunks::<2>().0 {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn non_finite_samples_become_silence() {
        let raw = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.5];
        let out = resample(&raw, 2, 44_100, 44_100);
        assert!(out.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn one_source_frame_still_produces_one_output_frame() {
        let out = resample(&[0.5], 1, 1_000, 1);
        assert_eq!(out, [0.5, 0.5]);
    }
}
