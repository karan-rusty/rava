use std::io::{self, Write as _};

use anyhow::{Context, Result, bail};

use crate::color::{Gradient, Rgb};
use crate::config::{ChannelMode, ColorMode, Config, Orientation};

const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Cell {
    ch: char,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
}

pub fn effective_bars(cfg: &Config, width: u16, height: u16) -> usize {
    let unit = (cfg.general.bar_width + cfg.general.bar_spacing).max(1) as usize;
    let spacing = cfg.general.bar_spacing as usize;
    let groups = match cfg.general.channel_mode {
        ChannelMode::Split | ChannelMode::Mirror => 2,
        ChannelMode::Average => 1,
    };
    let capacity = match cfg.output.orientation {
        Orientation::Vertical => {
            let segment = (width as usize / groups).max(1);
            (segment + spacing) / unit
        }
        Orientation::Horizontal => {
            let available = height as usize;
            (available + spacing) / unit
        }
    };
    let requested = if cfg.general.bars == 0 {
        capacity
    } else {
        cfg.general.bars
    };
    requested.min(capacity).clamp(1, 512)
}

pub struct Renderer {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    buffer: String,
    gradient: Gradient,
    bars: usize,
    bar_width: usize,
}

impl Renderer {
    pub fn new(cfg: &Config) -> Result<Self> {
        let (columns, rows) =
            crossterm::terminal::size().context("could not read terminal size")?;
        let width = columns.max(1) as usize;
        let height = rows.max(1) as usize;
        let bars = effective_bars(cfg, columns.max(1), rows.max(1));
        let gradient = build_gradient(cfg)?;
        Ok(Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            buffer: String::with_capacity(width * height * 8),
            gradient,
            bars,
            bar_width: cfg.general.bar_width.max(1) as usize,
        })
    }

    pub fn draw(&mut self, values: &[f32]) -> Result<()> {
        self.cells.fill(Cell::default());
        self.draw_bars(values);
        self.compose();
        let mut stdout = io::stdout();
        stdout
            .write_all(self.buffer.as_bytes())
            .context("could not write frame")?;
        stdout.flush().context("could not flush frame")?;
        Ok(())
    }

    fn draw_bars(&mut self, values: &[f32]) {
        let bars = values.len().min(self.bars).max(1);
        let draw_width = bars * self.bar_width;
        let x0 = self.width.saturating_sub(draw_width) / 2;
        for (bar, value) in values.iter().take(bars).enumerate() {
            let total = value.clamp(0.0, 1.0) * self.height as f32;
            let full = total.floor() as usize;
            let fraction = (total - full as f32).clamp(0.0, 1.0);
            for row in 0..=full {
                if row >= self.height {
                    break;
                }
                let y = self.height - 1 - row;
                let (ch, top) = if row < full {
                    ('█', (row + 1) as f32 / self.height as f32)
                } else if fraction > 0.01 {
                    let index = ((fraction * 8.0).ceil() as usize).clamp(1, 8);
                    (BLOCKS[index], (row as f32 + fraction) / self.height as f32)
                } else {
                    break;
                };
                let fg = self.gradient.at(top);
                self.put(x0 + bar * self.bar_width, y, ch, Some(fg));
            }
        }
    }

    fn put(&mut self, x: usize, y: usize, ch: char, fg: Option<Rgb>) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = Cell { ch, fg, bg: None };
        }
    }

    fn compose(&mut self) {
        self.buffer.clear();
        for y in 0..self.height {
            let _ = std::fmt::Write::write_fmt(&mut self.buffer, format_args!("\x1b[{};1H", y + 1));
            let mut last_fg = None;
            let mut last_bg = None;
            for x in 0..self.width {
                let cell = self.cells[y * self.width + x];
                if cell.fg != last_fg {
                    match cell.fg {
                        Some(color) => {
                            self.buffer.push_str("\x1b[");
                            color.write_fg(&mut self.buffer);
                            self.buffer.push('m');
                        }
                        None => self.buffer.push_str("\x1b[39m"),
                    }
                    last_fg = cell.fg;
                }
                if cell.bg != last_bg {
                    match cell.bg {
                        Some(color) => {
                            self.buffer.push_str("\x1b[");
                            color.write_bg(&mut self.buffer);
                            self.buffer.push('m');
                        }
                        None => self.buffer.push_str("\x1b[49m"),
                    }
                    last_bg = cell.bg;
                }
                self.buffer.push(cell.ch);
            }
        }
        self.buffer.push_str("\x1b[0m")
    }
}

fn build_gradient(cfg: &Config) -> Result<Gradient> {
    let stops = match cfg.output.color_mode {
        ColorMode::Solid => vec![Rgb::parse(&cfg.output.color).context("output.color")?],
        ColorMode::Gradient => {
            if cfg.output.gradient.is_empty() {
                bail!("output.gradient must list at least one color");
            }
            cfg.output
                .gradient
                .iter()
                .map(|stop| {
                    Rgb::parse(stop).with_context(|| format!("output.gradient entry `{stop}`"))
                })
                .collect::<Result<Vec<_>>>()?
        }
    };
    Gradient::new(stops)
}
