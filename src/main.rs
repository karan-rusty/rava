use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use std::io;
use std::time::{Duration, Instant};

mod analyzer;
mod audio;
mod color;
mod render;

use audio::Capture as _;

fn main() {
    if let Err(error) = run() {
        eprintln!("rava: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let _terminal = Terminal::enter()?;
    let mut source = audio::TestSource::new(44_100);
    let mut analyzer = analyzer::Analyzer::new(source.sample_rate());
    let mut renderer = render::Renderer::new()?;
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
