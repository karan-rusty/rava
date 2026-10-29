use std::io::{self, Write as _};

use anyhow::{Context, Result, bail};

use crate::color::{Gradient, Rgb};
use crate::config::{ChannelMode, ColorMode, Config, Orientation};

const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Braille cells filled from the bottom, in quarter steps.
const BRAILLE_UP: [char; 3] = ['⣀', '⣤', '⣶'];
/// Braille cells filled from the top, in quarter steps.
const BRAILLE_DOWN: [char; 3] = ['⠉', '⠛', '⠿'];
/// A fully lit braille cell.
const BRAILLE_FULL: char = '⣿';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: None,
            bg: None,
        }
    }
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
    mirror: bool,
    spectrum_speed: f32,
}

impl Renderer {
    pub fn new(cfg: &Config) -> Result<Self> {
        let (columns, rows) =
            crossterm::terminal::size().context("could not read terminal size")?;
        Self::with_size(cfg, columns, rows)
    }

    fn with_size(cfg: &Config, columns: u16, rows: u16) -> Result<Self> {
        let width = columns.max(1) as usize;
        let height = rows.max(1) as usize;
        let bars = effective_bars(cfg, columns.max(1), rows.max(1));
        let gradient = build_gradient(cfg)?;
        let spectrum_speed = if cfg.spectrum.speed.is_finite() {
            cfg.spectrum.speed
        } else {
            0.0
        };
        Ok(Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            buffer: String::with_capacity(width * height * 8),
            gradient,
            bars,
            bar_width: cfg.general.bar_width.max(1) as usize,
            mirror: cfg.spectrum.mirror,
            spectrum_speed,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Draw the wide eighth-block bars.
    pub fn draw_bars(&mut self, values: &[f32]) -> Result<()> {
        self.cells.fill(Cell::default());
        self.paint_bars(values);
        self.finish()
    }

    /// Draw the NoctaVox braille spectrum. `values` has one entry per column.
    pub fn draw_spectrum(&mut self, values: &[f32], elapsed: f32) -> Result<()> {
        self.cells.fill(Cell::default());
        self.paint_spectrum(values, elapsed);
        self.finish()
    }

    fn paint_bars(&mut self, values: &[f32]) {
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

    fn paint_spectrum(&mut self, values: &[f32], elapsed: f32) {
        let mirror = self.mirror;
        // Each half spans half the screen when mirrored, otherwise all of it.
        let span = if mirror {
            self.height as f32 / 2.0
        } else {
            self.height as f32
        };
        let (up_base, down_base) = if mirror {
            (self.height / 2, self.height.div_ceil(2))
        } else {
            (self.height, self.height)
        };
        let width = self.width;

        for (x, value) in values.iter().take(width).enumerate() {
            let value = value.clamp(0.0, 1.0);
            if value <= 0.0 {
                continue;
            }

            // NoctaVox color: a gradient across the screen, dimmed toward
            // black for quiet bars (but never fully dark).
            let position = x as f32 / width as f32;
            let color = self
                .gradient
                .at_wrapped(position, elapsed, self.spectrum_speed)
                .scale(value.max(0.35));

            let magnitude = value * span;
            let full = magnitude.floor() as usize;
            let fraction = magnitude - full as f32;

            for row in 0..full {
                if let Some(y) = up_base.checked_sub(row + 1) {
                    self.put(x, y, BRAILLE_FULL, Some(color));
                }
            }
            if fraction > 0.01
                && let Some(y) = up_base.checked_sub(full + 1)
            {
                self.put(x, y, braille_partial(fraction, false), Some(color));
            }

            if mirror {
                for row in 0..full {
                    self.put(x, down_base + row, BRAILLE_FULL, Some(color));
                }
                if fraction > 0.01 {
                    self.put(
                        x,
                        down_base + full,
                        braille_partial(fraction, true),
                        Some(color),
                    );
                }
            }
        }
    }

    fn put(&mut self, x: usize, y: usize, ch: char, fg: Option<Rgb>) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = Cell { ch, fg, bg: None };
        }
    }

    fn finish(&mut self) -> Result<()> {
        self.compose();
        let mut stdout = io::stdout();
        stdout
            .write_all(self.buffer.as_bytes())
            .context("could not write frame")?;
        stdout.flush().context("could not flush frame")?;
        Ok(())
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

/// Pick the braille glyph whose lit dots best match `fraction` of a cell.
fn braille_partial(fraction: f32, from_top: bool) -> char {
    let dots = ((fraction.clamp(0.0, 1.0) * 4.0).round() as usize).clamp(1, 4);
    if dots == 4 {
        return BRAILLE_FULL;
    }
    match from_top {
        true => BRAILLE_DOWN[dots - 1],
        false => BRAILLE_UP[dots - 1],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_renderer(width: u16, height: u16) -> Renderer {
        Renderer::with_size(&Config::default(), width, height).unwrap()
    }

    fn cell(renderer: &Renderer, x: usize, y: usize) -> Cell {
        renderer.cells[y * renderer.width + x]
    }

    #[test]
    fn full_spectrum_column_reaches_the_top() {
        let mut renderer = test_renderer(4, 8);
        renderer.paint_spectrum(&[1.0], 0.0);

        for y in 0..8 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿');
        }
        assert_eq!(cell(&renderer, 1, 7).ch, ' ');
    }

    #[test]
    fn spectrum_partial_heights_use_braille_quarters() {
        let mut renderer = test_renderer(4, 8);
        renderer.paint_spectrum(&[0.5], 0.0);

        // Exactly half the height is four full cells.
        assert_eq!(cell(&renderer, 0, 3).ch, ' ');
        for y in 4..8 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿');
        }

        let mut renderer = test_renderer(4, 8);
        renderer.paint_spectrum(&[0.55], 0.0);
        assert_eq!(cell(&renderer, 0, 3).ch, '⣤');

        // A nearly full cell rounds up to a full braille cell.
        let mut renderer = test_renderer(4, 10);
        renderer.paint_spectrum(&[0.99], 0.0);
        for y in 0..10 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿', "row {y}");
        }
    }

    #[test]
    fn mirrored_spectrum_grows_from_the_middle() {
        let mut config = Config::default();
        config.spectrum.mirror = true;
        let mut renderer = Renderer::with_size(&config, 4, 8).unwrap();
        renderer.paint_spectrum(&[0.5], 0.0);

        for y in 2..6 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿', "row {y}");
        }
        assert_eq!(cell(&renderer, 0, 1).ch, ' ');
        assert_eq!(cell(&renderer, 0, 6).ch, ' ');
    }

    #[test]
    fn spectrum_colors_bars_and_leaves_gaps_empty() {
        let mut renderer = test_renderer(8, 8);
        renderer.paint_spectrum(&[0.0, 1.0, 0.0], 0.0);

        assert_eq!(cell(&renderer, 0, 7).ch, ' ');
        assert_eq!(cell(&renderer, 2, 7).ch, ' ');
        let bar = cell(&renderer, 1, 7);
        assert_eq!(bar.ch, '⣿');
        assert!(bar.fg.is_some());
    }

    #[test]
    fn spectrum_gradient_scrolls_when_speed_is_set() {
        let mut config = Config::default();
        config.spectrum.speed = 1.0;
        let mut first = Renderer::with_size(&config, 4, 8).unwrap();
        let mut later = Renderer::with_size(&config, 4, 8).unwrap();
        first.paint_spectrum(&[1.0], 0.0);
        later.paint_spectrum(&[1.0], 0.25);

        assert_ne!(
            cell(&first, 0, 7).fg,
            cell(&later, 0, 7).fg,
            "a scrolling ramp changes color over time"
        );
    }

    #[test]
    fn bars_still_draw_full_block_columns() {
        let mut renderer = test_renderer(4, 8);
        renderer.paint_bars(&[1.0, 1.0]);

        // Each bar is one cell wide inside its `bar_width` slot.
        for y in 0..8 {
            assert_eq!(cell(&renderer, 0, y).ch, '█');
            assert_eq!(cell(&renderer, 2, y).ch, '█');
            assert_eq!(cell(&renderer, 1, y).ch, ' ');
        }
    }
}
