use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use audio::{FileSource, LiveSource, SilenceGate, TestSource};

mod analyzer;
mod audio;
mod color;
mod config;
mod render;
mod spectrum;

/// Playback rate the sources resample to.
const SAMPLE_RATE: u32 = 44_100;

#[derive(Debug, Parser)]
#[command(name = "rava", version, about = "A console audio visualizer")]
struct Args {
    /// TOML configuration file.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Decode and loop a WAV file instead of capturing live audio.
    #[arg(long, value_name = "WAV", conflicts_with_all = ["source", "demo"])]
    file: Option<String>,
    /// Capture system audio from this PulseAudio/PipeWire source.
    ///
    /// `auto` (the default) uses the monitor of the default output.
    #[arg(short, long, value_name = "SOURCE", conflicts_with_all = ["file", "demo"])]
    source: Option<String>,
    /// Play the built-in synthetic loop instead of capturing audio.
    #[arg(long, conflicts_with_all = ["file", "source"])]
    demo: bool,
    /// Visualizer to start with; press `w` to cycle at runtime.
    #[arg(short, long, value_enum, value_name = "MODE")]
    mode: Option<config::Mode>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rava: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let config = config::Config::load(args.config.as_deref())?;
    let mut mode = args.mode.unwrap_or(config.general.mode);

    // Without an explicit file or demo, capture whatever the machine is
    // playing; `--source` (or `[input] source`) picks a specific source.
    let live = args.file.is_none() && !args.demo;
    let mut source: Box<dyn audio::Capture> = if let Some(path) = args.file.as_deref() {
        Box::new(FileSource::open(path, SAMPLE_RATE, true)?)
    } else if args.demo {
        Box::new(TestSource::new(SAMPLE_RATE))
    } else {
        let requested = args.source.as_deref().unwrap_or(&config.input.source);
        Box::new(LiveSource::open(requested, SAMPLE_RATE)?)
    };
    let mut gate = live.then(|| {
        SilenceGate::new(
            config.input.threshold_db,
            Duration::from_millis(config.input.idle_after_ms),
        )
    });

    let _terminal = Terminal::enter()?;
    let mut spectrum = spectrum::Spectrum::new(config.spectrum.decay);
    let mut analyzer = analyzer::Analyzer::new(source.sample_rate());
    let mut renderer = render::Renderer::new(&config)?;
    let mut samples = Vec::new();
    let mut mono = Vec::new();
    let mut previous = Instant::now();
    let started = Instant::now();
    let mut drawing = false;

    loop {
        if event::poll(Duration::ZERO).context("could not poll terminal events")?
            && let Event::Key(key) = event::read().context("could not read terminal event")?
            && key.kind != KeyEventKind::Release
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('w') => mode = mode.next(),
                _ => {}
            }
        }
        let now = Instant::now();
        let dt = now.duration_since(previous).as_secs_f32().min(0.1);
        previous = now;

        source.read(&mut samples)?;

        // Both analysers want mono frames; the bars used to see interleaved
        // stereo, which put every band at twice its real frequency.
        mono.clear();
        mono.extend(
            samples
                .as_chunks::<2>()
                .0
                .iter()
                .map(|frame| (frame[0] + frame[1]) * 0.5),
        );
        analyzer.push(&mono);
        spectrum.push(&mono);
        analyzer.update(dt);
        spectrum.update(source.sample_rate());

        // Live input starts blank and blanks again after a quiet spell; the
        // gate sees the raw (not mono-folded) stream so out-of-phase channels
        // cannot look like silence.
        let active = match gate.as_mut() {
            Some(gate) => gate.observe(&samples, Instant::now()),
            None => true,
        };

        if active {
            match mode {
                config::Mode::Bars => renderer.draw_bars(analyzer.values())?,
                config::Mode::Spectrum => {
                    spectrum.remap_display(renderer.width());
                    renderer
                        .draw_spectrum(spectrum.display_bins(), started.elapsed().as_secs_f32())?;
                }
            }
        } else if drawing {
            // One blank frame is enough; while idle there is nothing to draw.
            renderer.draw_idle()?;
        }
        drawing = active;

        let elapsed = now.elapsed();
        let frame_time = Duration::from_secs_f32(1.0 / 60.0);
        if elapsed < frame_time {
            std::thread::sleep(frame_time - elapsed);
        }
    }
    Ok(())
}

struct Terminal;

impl Terminal {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("could not enable raw mode")?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All)
        )
        .context("could not enter alternate screen")?;
        Ok(Self)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_is_optional_so_the_config_can_decide() {
        let args = Args::try_parse_from(["rava"]).unwrap();
        assert!(args.mode.is_none());
    }

    #[test]
    fn mode_accepts_both_visualizers() {
        let spectrum = Args::try_parse_from(["rava", "--mode", "spectrum"]).unwrap();
        assert_eq!(spectrum.mode, Some(config::Mode::Spectrum));

        let bars = Args::try_parse_from(["rava", "-m", "bars"]).unwrap();
        assert_eq!(bars.mode, Some(config::Mode::Bars));
    }

    #[test]
    fn unknown_mode_is_rejected() {
        assert!(Args::try_parse_from(["rava", "--mode", "waveform"]).is_err());
    }

    #[test]
    fn live_capture_is_the_default() {
        let args = Args::try_parse_from(["rava"]).unwrap();
        assert!(args.file.is_none());
        assert!(args.source.is_none());
        assert!(!args.demo);
    }

    #[test]
    fn parses_an_explicit_source() {
        let args = Args::try_parse_from(["rava", "--source", "alsa_output.usb.monitor"]).unwrap();
        assert_eq!(args.source.as_deref(), Some("alsa_output.usb.monitor"));
    }

    #[test]
    fn input_modes_are_mutually_exclusive() {
        assert!(Args::try_parse_from(["rava", "--file", "a.wav", "--source", "auto"]).is_err());
        assert!(Args::try_parse_from(["rava", "--file", "a.wav", "--demo"]).is_err());
        assert!(Args::try_parse_from(["rava", "--demo", "--source", "auto"]).is_err());
    }
}
