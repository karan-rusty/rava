use std::io::{self, Write as _};

use anyhow::{Context, Result};

use crate::color::{Gradient, Rgb};

const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    fg: Option<Rgb>,
}

impl Default for Cell {
    fn default() -> Self {
        Self { ch: ' ', fg: None }
    }
}

pub struct Renderer {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    buffer: String,
    gradient: Gradient,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let (width, height) =
            crossterm::terminal::size().context("could not read terminal size")?;
        let width = width.max(1) as usize;
        let height = height.max(1) as usize;
        let gradient = Gradient::new(vec![
            Rgb::new(0, 229, 255),
            Rgb::new(41, 121, 255),
            Rgb::new(124, 77, 255),
            Rgb::new(255, 64, 129),
        ])?;
        Ok(Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            buffer: String::with_capacity(width * height * 8),
            gradient,
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
        let bars = values.len().min((self.width / 2).max(1));
        let draw_width = bars * 2;
        let x0 = (self.width - draw_width) / 2;
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
                self.put(x0 + bar * 2, y, ch, Some(fg));
            }
        }
    }

    fn put(&mut self, x: usize, y: usize, ch: char, fg: Option<Rgb>) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = Cell { ch, fg };
        }
    }

    fn compose(&mut self) {
        self.buffer.clear();
        for y in 0..self.height {
            let _ = std::fmt::Write::write_fmt(&mut self.buffer, format_args!("\x1b[{};1H", y + 1));
            let mut last = None;
            for x in 0..self.width {
                let cell = self.cells[y * self.width + x];
                if cell.fg != last {
                    match cell.fg {
                        Some(color) => {
                            self.buffer.push_str("\x1b[");
                            color.write_fg(&mut self.buffer);
                            self.buffer.push('m');
                        }
                        None => self.buffer.push_str("\x1b[39m"),
                    }
                    last = cell.fg;
                }
                self.buffer.push(cell.ch);
            }
        }
        self.buffer.push_str("\x1b[0m")
    }
}
