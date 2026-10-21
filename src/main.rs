use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use std::io::{self, Write};
use std::time::{Duration, Instant};

mod analyzer;
mod audio;

fn main () {
    if let Err(error) = run() {
        eprintln!("rava: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let _terminal = Terminal::enter()?;
    let mut source = audio::TestTone::new(44_100);
    let mut analyzer = analyzer::Analyzer::new(44_100);
    let mut samples = Vec::new();
    let mut previous = Instant::now();

    loop {
        if event::poll(Duration::ZERO).context("could not poll terminal events")? {
            if let Event::Key(key) = event::read().context("could not read terminal event")? {
                if key.kind != KeyEventKind::Release && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
        let now = Instant::now();
        let dt = now.duration_since(previous).as_secs_f32().min(0.1);
        previous = now;

        source.read(735, &mut samples);
        analyzer.push(&samples);
        analyzer.update(dt);
        draw(analyzer.values())?;

        let elapsed = now.elapsed();
        let frame_time = Duration::from_secs_f32(1.0 / 60.0);
        if elapsed < frame_time {
            std::thread::sleep(frame_time - elapsed);
        }
    }
    Ok(())
}

fn draw(values: &[f32]) -> Result<()> {
    let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
    let width = width.max(1) as usize;
    let height = height.max(2) as usize;
    let bars = values.len().min(width / 2).max(1);
    let draw_width = bars * 2;
    let x0 = width.saturating_sub(draw_width) / 2;
    let mut cells = vec![(' ', None::<(u8, u8, u8)>); width * height];

    for (bar, value) in values.iter().take(bars).enumerate() {
        let total = value.clamp(0.0, 1.0) * height as f32;
        let full = total.floor() as usize;
        let fraction = total - full as f32;
        for row in 0..=full {
            if row >= height {
                break;
            }
            let y = height - 1 - row;
            let (ch, level) = if row < full {
('█', (row + 1) as f32 / height as f32)
            } else if fraction > 0.01 {
                let index = ((fraction * 8.0).ceil() as usize).clamp(1,8);
(
[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][index],
                    total / height as f32,
            )
            } else {
                break;
            };
            cells[y * width + x0 + bar * 2] = (ch, Some(color(level)));
        }
    }
    let mut stdout = io::stdout().lock();
    let mut out = String::with_capacity(width * height * 8);
    out.push_str("\x1b[H");
    for y in 0..height {
        out.push_str(&format!("\x1b[{};1H",y + 1));
        let mut last = None;
        for x in 0..width {
            let (ch, fg) = cells[y * width + x];
            if fg != last {
                match fg {
                    Some((r, g, b)) => out.push_str(&format!("\x1b[38;2;{r};{g};{b}m")),
                    None => out.push_str("\x1b[39m"),
                }
                last = fg;
            }
            out.push(ch);
        }
    }
    out.push_str("\x1b[0m");
    stdout.write_all(out.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

fn color(t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        let k = t * 2.0;
        (
            (22.0 + 19.0 * k) as u8,
            (22.0 + 19.0 * k) as u8,
            (84.0 + 171.0 * k) as u8,
        )
    } else {
        let k = (t - 0.5) * 2.0;
        (
            (41.0 + 214.0 * k) as u8,
            (41.0 * (1.0 - k)) as u8,
            (255.0 * (1.0 - k) + 129.0 * k) as u8,
        )
    }
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
