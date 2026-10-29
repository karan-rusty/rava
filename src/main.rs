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

use audio::{FileSource, TestSource};

mod analyzer;
mod audio;
mod color;
mod config;
mod render;

/// Playback rate the sources resample to.
const SAMPLE_RATE: u32 = 44_100;

#[derive(Debug, Parser)]
#[command(name = "rava", version, about = "A console audio visualizer")]
struct Args {
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "WAV")]
    file: Option<String>,
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
    let mut source: Box<dyn audio::Capture> = match args.file.as_deref() {
        Some(path) => Box::new(FileSource::open(path, SAMPLE_RATE, true)?),
        None => Box::new(TestSource::new(SAMPLE_RATE)),
    };

    let _terminal = Terminal::enter()?;
    let mut analyzer = analyzer::Analyzer::new(source.sample_rate());
    let mut renderer = render::Renderer::new(&config)?;
    let mut samples = Vec::new();
    let mut previous = Instant::now();

    loop {
        if event::poll(Duration::ZERO).context("could not poll terminal events")? {
            if let Event::Key(key) = event::read().context("could not read terminal event")? {
                if key.kind != KeyEventKind::Release
                    && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                {
                    break;
                }
            }
        }
        let now = Instant::now();
        let dt = now.duration_since(previous).as_secs_f32().min(0.1);
        previous = now;

        source.read(&mut samples)?;
        analyzer.push(&samples);
        analyzer.update(dt);
        renderer.draw(analyzer.values())?;

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
