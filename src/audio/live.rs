//! Live system-audio capture through PulseAudio/PipeWire's `parec`.
//!
//! `parec` gives us the monitor source of the default sink — everything the
//! machine is playing — as raw little-endian f32 stereo, paced by the audio
//! server itself. Keeping the capture in a child process avoids linking
//! libpulse while still working on both PulseAudio and PipeWire.

use std::io::{self, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{Context, Result, bail};

use super::{CHUNK_FRAMES, Capture};

const CHANNELS: usize = 2;
const BYTES_PER_SAMPLE: usize = std::mem::size_of::<f32>();
const FRAME_BYTES: usize = CHANNELS * BYTES_PER_SAMPLE;

/// Requested capture latency. Small enough to feel live, large enough to avoid
/// dropping frames when the visualizer is busy.
const LATENCY_MS: u32 = 40;

pub struct LiveSource {
    child: Child,
    stream: ChildStdout,
    stderr: StderrTail,
    device: String,
    rate: u32,
    bytes: Vec<u8>,
}

impl LiveSource {
    /// Capture `source`, or the default sink's monitor when it is `"auto"`.
    pub fn open(source: &str, rate: u32) -> Result<Self> {
        let device = resolve_source(source)?;
        ensure_source_exists(&device)?;
        let mut command = Command::new("parec");
        command
            .args(parec_args(&device, rate))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .with_context(|| "could not start `parec`; install pulseaudio-utils")?;

        let stream = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        Ok(Self {
            child,
            stream,
            stderr: StderrTail::new(stderr),
            device,
            rate,
            bytes: vec![0; CHUNK_FRAMES * FRAME_BYTES],
        })
    }

    /// Wait for the child to exit and collect whatever it wrote to stderr.
    fn failure_detail(&mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stderr.finish();
        let text = self.stderr.text();
        if text.is_empty() {
            String::new()
        } else {
            format!(": {text}")
        }
    }
}

impl Capture for LiveSource {
    fn read(&mut self, out: &mut Vec<f32>) -> Result<usize> {
        let mut filled = 0;
        while filled < self.bytes.len() {
            match self.stream.read(&mut self.bytes[filled..]) {
                Ok(0) => {
                    let detail = self.failure_detail();
                    bail!("audio capture from `{}` stopped{detail}", self.device);
                }
                Ok(count) => filled += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    let detail = self.failure_detail();
                    return Err(error).with_context(|| {
                        format!("could not read audio from `{}`{detail}", self.device)
                    });
                }
            }
        }

        out.clear();
        out.reserve(CHUNK_FRAMES * CHANNELS);
        for sample in self.bytes.as_chunks::<BYTES_PER_SAMPLE>().0 {
            out.push(f32::from_le_bytes([
                sample[0], sample[1], sample[2], sample[3],
            ]));
        }
        Ok(CHUNK_FRAMES)
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }
}

impl Drop for LiveSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stderr.finish();
    }
}

/// The command line used to open a raw float capture stream.
fn parec_args(device: &str, rate: u32) -> Vec<String> {
    vec![
        "--raw".to_owned(),
        "--format=float32le".to_owned(),
        format!("--rate={rate}"),
        format!("--channels={CHANNELS}"),
        format!("--latency-msec={LATENCY_MS}"),
        "--client-name=rava".to_owned(),
        format!("--device={device}"),
    ]
}

fn resolve_source(source: &str) -> Result<String> {
    if source == "auto" {
        return default_monitor();
    }
    if source.trim().is_empty() {
        bail!("input.source must not be empty");
    }
    Ok(source.to_owned())
}

/// `<default sink>.monitor`, or parec's own default-monitor alias when pactl
/// is not available.
fn default_monitor() -> Result<String> {
    if let Some(sink) = default_sink() {
        return Ok(format!("{sink}.monitor"));
    }
    Ok("@DEFAULT_MONITOR@".to_owned())
}

fn default_sink() -> Option<String> {
    let output = Command::new("pactl")
        .args(["get-default-sink"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sink = String::from_utf8(output.stdout).ok()?;
    let sink = sink.trim();
    (!sink.is_empty()).then(|| sink.to_owned())
}

/// Reject typos instead of letting PipeWire fall back to the default source.
/// Skips the check when `pactl` is unavailable, leaving the decision to parec.
fn ensure_source_exists(source: &str) -> Result<()> {
    if source.starts_with('@') {
        return Ok(());
    }
    let Some(sources) = available_sources() else {
        return Ok(());
    };
    if sources.iter().any(|name| name == source) {
        return Ok(());
    }
    bail!("audio source `{source}` does not exist; `pactl list short sources` lists them")
}

fn available_sources() -> Option<Vec<String>> {
    let output = Command::new("pactl")
        .args(["list", "short", "sources"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(parse_source_names(&text))
}

fn parse_source_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

/// Drains `parec`'s stderr so a chatty child can never block on a full pipe,
/// while keeping the tail around for an error message.
struct StderrTail {
    text: Arc<Mutex<String>>,
    thread: Option<JoinHandle<()>>,
}

impl StderrTail {
    fn new(mut stderr: ChildStderr) -> Self {
        const LIMIT: usize = 4 * 1024;
        let text = Arc::new(Mutex::new(String::new()));
        let slot = Arc::clone(&text);
        let thread = std::thread::spawn(move || {
            let mut buffer = [0u8; 512];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let chunk = String::from_utf8_lossy(&buffer[..count]);
                        let mut slot = slot.lock().unwrap_or_else(|poison| poison.into_inner());
                        if slot.len() + chunk.len() > LIMIT {
                            slot.clear();
                        }
                        slot.push_str(&chunk);
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        });
        Self {
            text,
            thread: Some(thread),
        }
    }

    fn text(&self) -> String {
        self.text
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .trim()
            .to_owned()
    }

    fn finish(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_resolves_to_the_default_sink_monitor() {
        assert_eq!(resolve_source("auto").unwrap(), default_monitor().unwrap());
    }

    #[test]
    fn explicit_sources_are_used_verbatim() {
        let source = "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor";
        assert_eq!(resolve_source(source).unwrap(), source);
    }

    #[test]
    fn empty_sources_are_rejected() {
        assert!(resolve_source("  ").is_err());
    }

    #[test]
    fn parses_source_names_from_pactl() {
        let text = "\
            33\teasyeffects_sink.monitor\tPipeWire\tfloat32le 2ch 48000Hz\tRUNNING\n\
            85\talsa_output.usb.analog-stereo.monitor\tPipeWire\ts16le 2ch 48000Hz\tRUNNING\n";
        assert_eq!(
            parse_source_names(text),
            [
                "easyeffects_sink.monitor",
                "alsa_output.usb.analog-stereo.monitor"
            ]
        );
    }

    #[test]
    fn arguments_request_raw_float_stereo_at_the_target_rate() {
        let args = parec_args("my.monitor", 44_100);
        assert_eq!(args[0], "--raw");
        assert!(args.contains(&"--format=float32le".to_owned()));
        assert!(args.contains(&"--rate=44100".to_owned()));
        assert!(args.contains(&"--channels=2".to_owned()));
        assert!(args.contains(&"--device=my.monitor".to_owned()));
    }
}
