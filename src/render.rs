use std::io::{self, Write as _};

use anyhow::{Context, Result, bail};

use crate::color::{Gradient, Rgb};
use crate::config::{ChannelMode, ColorMode, Config, Orientation};

const VERTICAL_BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Left-to-right eighth blocks, for horizontal bars.
const HORIZONTAL_BLOCKS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// Braille dot bits for each dot row of a cell, both dot columns together.
const BRAILLE_ROW_BITS: [u32; 4] = [0x09, 0x12, 0x24, 0xc0];
/// Height of the spectrum band as a fraction of the terminal. NoctaVox pads its
/// fullscreen visualizer by a quarter of the height at the top and bottom.
const SPECTRUM_HEIGHT: f32 = 0.5;

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

fn effective_bars(cfg: &Config, width: u16, height: u16) -> usize {
    let bar_width = cfg.general.bar_width.max(1) as usize;
    let spacing = cfg.general.bar_spacing as usize;
    let unit = (bar_width + spacing).max(1);
    let groups = match cfg.general.channel_mode {
        ChannelMode::Split | ChannelMode::Mirror => 2,
        ChannelMode::Average => 1,
    };
    let axis = match cfg.output.orientation {
        Orientation::Vertical => width as usize,
        Orientation::Horizontal => height as usize,
    };
    let capacity = (axis / groups).max(1).saturating_add(spacing) / unit;
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
    bar_spacing: usize,
    orientation: Orientation,
    channel_mode: ChannelMode,
    mirror: bool,
    spectrum_speed: f32,
}

impl Renderer {
    pub fn new(cfg: &Config) -> Result<Self> {
        let (columns, rows) =
            crossterm::terminal::size().context("could not read terminal size")?;
        Self::with_size(cfg, columns, rows)
    }

    pub(crate) fn with_size(cfg: &Config, columns: u16, rows: u16) -> Result<Self> {
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
            bar_spacing: cfg.general.bar_spacing as usize,
            orientation: cfg.output.orientation,
            channel_mode: cfg.general.channel_mode,
            mirror: cfg.spectrum.mirror,
            spectrum_speed,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Blank the whole screen. Used when live capture has gone quiet.
    pub fn draw_idle(&mut self) -> Result<()> {
        self.clear();
        self.finish()
    }

    /// Draw the wide eighth-block bars for the configured channel layout.
    pub fn draw_bars(&mut self, mono: &[f32], left: &[f32], right: &[f32]) -> Result<()> {
        self.clear();
        match self.channel_mode {
            ChannelMode::Average => self.paint_bars(mono, 0),
            ChannelMode::Split => {
                self.paint_bars(left, 0);
                self.paint_bars(right, 1);
            }
            ChannelMode::Mirror => {
                self.paint_bars(left, 0);
                self.paint_bars(left, 1);
            }
        }
        self.finish()
    }

    /// Draw the NoctaVox braille spectrum. `values` has one entry per column.
    pub fn draw_spectrum(&mut self, values: &[f32], elapsed: f32) -> Result<()> {
        self.clear();
        self.paint_spectrum(values, elapsed);
        self.finish()
    }

    fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    fn paint_bars(&mut self, values: &[f32], group: usize) {
        if values.is_empty() {
            return;
        }
        let groups = match self.channel_mode {
            ChannelMode::Average => 1,
            ChannelMode::Split | ChannelMode::Mirror => 2,
        };
        let bars = values.len().min(self.bars);
        let unit = self.bar_width + self.bar_spacing;
        let span = (bars - 1) * unit + self.bar_width;
        let mirrored = self.channel_mode == ChannelMode::Mirror && group == 1;
        // Mirror the finished first group across the screen instead of laying
        // out a second group whose centering may differ by a cell.
        let layout_group = if mirrored { 0 } else { group };
        let (start, end) = match self.orientation {
            Orientation::Vertical => (
                layout_group * self.width / groups,
                (layout_group + 1) * self.width / groups,
            ),
            Orientation::Horizontal => (
                layout_group * self.height / groups,
                (layout_group + 1) * self.height / groups,
            ),
        };
        let origin = start + (end - start).saturating_sub(span) / 2;
        let axis = match self.orientation {
            Orientation::Vertical => self.width,
            Orientation::Horizontal => self.height,
        };

        for (index, value) in values.iter().take(bars).enumerate() {
            let position = origin + index * unit;
            let position = if mirrored {
                axis - 1 - position
            } else {
                position
            };
            match self.orientation {
                Orientation::Vertical => self.paint_vertical_bar(position, *value),
                Orientation::Horizontal => self.paint_horizontal_bar(position, *value),
            }
        }
    }

    fn paint_vertical_bar(&mut self, x: usize, value: f32) {
        let total = value.clamp(0.0, 1.0) * self.height as f32;
        let full = total.floor() as usize;
        let fraction = (total - full as f32).clamp(0.0, 1.0);
        for row in 0..=full {
            if row >= self.height {
                break;
            }
            let y = self.height - 1 - row;
            let (ch, along) = if row < full {
                ('█', (row + 1) as f32 / self.height as f32)
            } else if fraction > 0.01 {
                let index = ((fraction * 8.0).ceil() as usize).clamp(1, 8);
                (
                    VERTICAL_BLOCKS[index],
                    (row as f32 + fraction) / self.height as f32,
                )
            } else {
                break;
            };
            let fg = self.gradient.at(along);
            self.put(x, y, ch, Some(fg));
        }
    }

    fn paint_horizontal_bar(&mut self, y: usize, value: f32) {
        let total = value.clamp(0.0, 1.0) * self.width as f32;
        let full = total.floor() as usize;
        let fraction = (total - full as f32).clamp(0.0, 1.0);
        for column in 0..=full {
            if column >= self.width {
                break;
            }
            let (ch, along) = if column < full {
                ('█', (column + 1) as f32 / self.width as f32)
            } else if fraction > 0.01 {
                let index = ((fraction * 8.0).ceil() as usize).clamp(1, 8);
                (
                    HORIZONTAL_BLOCKS[index],
                    (column as f32 + fraction) / self.width as f32,
                )
            } else {
                break;
            };
            let fg = self.gradient.at(along);
            self.put(column, y, ch, Some(fg));
        }
    }

    fn paint_spectrum(&mut self, values: &[f32], elapsed: f32) {
        let dots = self.height * 4;
        let band = ((dots as f32 * SPECTRUM_HEIGHT) as usize).clamp(1, dots);
        let top = (dots - band) / 2;
        let bottom = top + band;
        let center = top + band / 2;
        let width = self.width;

        for (x, value) in values.iter().take(width).enumerate() {
            let value = value.clamp(0.0, 1.0);

            // NoctaVox color: a gradient across the screen, dimmed toward
            // black for quiet bars (but never fully dark).
            let position = x as f32 / width as f32;
            let color = self
                .gradient
                .at_wrapped(position, elapsed, self.spectrum_speed)
                .scale(value.max(0.35));

            // NoctaVox draws a line between two rounded canvas points. A zero
            // line still marks its endpoint, so every column keeps the two
            // centre dots and the midline stays continuous.
            let (first, last) = if self.mirror {
                let reach = value * band as f32 / 2.0;
                let lo = (center as f32 - reach).round().max(top as f32);
                let hi = (center as f32 + reach).round().min((bottom - 1) as f32);
                (lo as usize, hi as usize)
            } else {
                let reach = value * band as f32;
                let first = (bottom as f32 - reach).round().max(top as f32);
                (first as usize, bottom - 1)
            };
            self.paint_dots(x, first, last + 1, color);
        }
    }

    /// Paint dot rows `first..last` of one column as braille dots.
    fn paint_dots(&mut self, x: usize, first: usize, last: usize, color: Rgb) {
        for y in first / 4..last.div_ceil(4) {
            let mut mask = 0;
            for (row, bits) in BRAILLE_ROW_BITS.iter().enumerate() {
                if (first..last).contains(&(y * 4 + row)) {
                    mask |= bits;
                }
            }
            if mask != 0 {
                let ch = char::from_u32(0x2800 | mask).expect("braille block");
                self.put(x, y, ch, Some(color));
            }
        }
    }

    fn put(&mut self, x: usize, y: usize, ch: char, fg: Option<Rgb>) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = Cell { ch, fg };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_renderer(width: u16, height: u16) -> Renderer {
        Renderer::with_size(&Config::default(), width, height).unwrap()
    }

    fn bottom_up_renderer(width: u16, height: u16) -> Renderer {
        let mut config = Config::default();
        config.spectrum.mirror = false;
        Renderer::with_size(&config, width, height).unwrap()
    }

    fn cell(renderer: &Renderer, x: usize, y: usize) -> Cell {
        renderer.cells[y * renderer.width + x]
    }

    #[test]
    fn full_spectrum_column_stays_inside_the_band() {
        let mut renderer = test_renderer(4, 8);
        renderer.paint_spectrum(&[1.0], 0.0);

        // The middle half of an 8-row terminal is rows 2..5.
        for y in 2..6 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿');
        }
        for y in [0, 1, 6, 7] {
            assert_eq!(cell(&renderer, 0, y).ch, ' ', "row {y}");
        }
        assert_eq!(cell(&renderer, 1, 4).ch, ' ');
    }

    #[test]
    fn spectrum_edges_use_partial_braille_cells() {
        // Half level fills the lower half of the band.
        let mut renderer = bottom_up_renderer(4, 8);
        renderer.paint_spectrum(&[0.5], 0.0);
        assert_eq!(cell(&renderer, 0, 3).ch, ' ');
        for y in 4..6 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿');
        }
        assert_eq!(cell(&renderer, 0, 6).ch, ' ');

        // A level that does not land on a cell boundary leaves a partial cell.
        let mut renderer = bottom_up_renderer(4, 8);
        renderer.paint_spectrum(&[0.6], 0.0);
        assert_eq!(cell(&renderer, 0, 3).ch, '⣤');

        // Both ends of the band can cut through a cell.
        let mut renderer = bottom_up_renderer(4, 10);
        renderer.paint_spectrum(&[0.99], 0.0);
        assert_eq!(cell(&renderer, 0, 2).ch, '⣤');
        assert_eq!(cell(&renderer, 0, 7).ch, '⠛');
        for y in 3..7 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿', "row {y}");
        }
        for y in [0, 1, 8, 9] {
            assert_eq!(cell(&renderer, 0, y).ch, ' ', "row {y}");
        }
    }

    #[test]
    fn default_spectrum_is_mirrored() {
        let mut renderer = test_renderer(4, 8);
        renderer.paint_spectrum(&[0.5], 0.0);

        for y in 3..5 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿', "row {y}");
        }
        // The rounded lower endpoint is the centre dot row of the next cell.
        assert_eq!(cell(&renderer, 0, 5).ch, '⠉');
        assert_eq!(cell(&renderer, 0, 2).ch, ' ');
    }

    #[test]
    fn mirrored_spectrum_grows_from_the_middle() {
        let mut config = Config::default();
        config.spectrum.mirror = true;
        let mut renderer = Renderer::with_size(&config, 4, 8).unwrap();
        renderer.paint_spectrum(&[0.5], 0.0);

        for y in 3..5 {
            assert_eq!(cell(&renderer, 0, y).ch, '⣿', "row {y}");
        }
        assert_eq!(cell(&renderer, 0, 5).ch, '⠉');
        assert_eq!(cell(&renderer, 0, 2).ch, ' ');
    }

    #[test]
    fn spectrum_colors_bars_and_keeps_a_midline() {
        let mut renderer = test_renderer(8, 8);
        renderer.paint_spectrum(&[0.0, 1.0, 0.0], 0.0);

        let bar = cell(&renderer, 1, 2);
        assert_eq!(bar.ch, '⣿');
        assert!(bar.fg.is_some());

        // Silent columns still show the two centre dots.
        assert_eq!(cell(&renderer, 0, 4).ch, '⠉');
        assert_eq!(cell(&renderer, 2, 4).ch, '⠉');
        assert_eq!(cell(&renderer, 0, 2).ch, ' ');
    }

    #[test]
    fn spectrum_keeps_a_centerline_on_zero_columns() {
        let mut renderer = test_renderer(4, 10);
        renderer.paint_spectrum(&[0.0], 0.0);

        assert_eq!(cell(&renderer, 0, 5).ch, '⠉');
        for y in [0, 1, 2, 3, 4, 6, 7, 8, 9] {
            assert_eq!(cell(&renderer, 0, y).ch, ' ', "row {y}");
        }
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
            cell(&first, 0, 4).fg,
            cell(&later, 0, 4).fg,
            "a scrolling ramp changes color over time"
        );
    }

    #[test]
    fn bars_still_draw_full_block_columns() {
        let mut renderer = test_renderer(4, 8);
        renderer.paint_bars(&[1.0, 1.0], 0);

        // Each bar is one cell wide inside its `bar_width` slot.
        for y in 0..8 {
            assert_eq!(cell(&renderer, 0, y).ch, '█');
            assert_eq!(cell(&renderer, 2, y).ch, '█');
            assert_eq!(cell(&renderer, 1, y).ch, ' ');
        }
    }

    #[test]
    fn bar_spacing_leaves_gaps() {
        let mut config = Config::default();
        config.general.bars = 3;
        config.general.bar_spacing = 1;
        let mut renderer = Renderer::with_size(&config, 12, 4).unwrap();
        renderer.paint_bars(&[1.0; 3], 0);

        for y in 0..4 {
            for x in [2, 5, 8] {
                assert_eq!(cell(&renderer, x, y).ch, '█', "bar at {x},{y}");
            }
            for x in [3, 4, 6, 7, 9] {
                assert_eq!(cell(&renderer, x, y).ch, ' ', "gap at {x},{y}");
            }
        }
    }

    #[test]
    fn horizontal_bars_grow_left_to_right() {
        let mut config = Config::default();
        config.general.bars = 2;
        config.output.orientation = Orientation::Horizontal;
        let mut renderer = Renderer::with_size(&config, 12, 8).unwrap();
        renderer.paint_bars(&[1.0, 0.5], 0);

        // Two bars, centered on rows 2 and 4; half height fills six columns.
        for x in 0..12 {
            assert_eq!(cell(&renderer, x, 2).ch, '█', "full bar at {x}");
            assert_eq!(cell(&renderer, x, 1).ch, ' ');
        }
        for x in 0..6 {
            assert_eq!(cell(&renderer, x, 4).ch, '█', "half bar at {x}");
        }
        for x in 6..12 {
            assert_eq!(cell(&renderer, x, 4).ch, ' ');
        }
    }

    #[test]
    fn horizontal_partial_bars_use_left_blocks() {
        let mut config = Config::default();
        config.general.bars = 1;
        config.general.bar_width = 1;
        config.output.orientation = Orientation::Horizontal;
        let mut renderer = Renderer::with_size(&config, 10, 4).unwrap();
        renderer.paint_bars(&[0.55], 0);

        // Five full cells and a half-filled sixth on the centered row.
        for x in 0..5 {
            assert_eq!(cell(&renderer, x, 1).ch, '█', "full cell {x}");
        }
        assert_eq!(cell(&renderer, 5, 1).ch, '▌');
        for x in 6..10 {
            assert_eq!(cell(&renderer, x, 1).ch, ' ');
        }
    }

    #[test]
    fn horizontal_split_stacks_the_channels() {
        let mut config = Config::default();
        config.general.bars = 2;
        config.general.bar_width = 1;
        config.general.channel_mode = ChannelMode::Split;
        config.output.orientation = Orientation::Horizontal;
        let mut renderer = Renderer::with_size(&config, 8, 8).unwrap();
        renderer.paint_bars(&[1.0, 0.0], 0);
        renderer.paint_bars(&[0.0, 1.0], 1);

        for x in 0..8 {
            assert_eq!(cell(&renderer, x, 1).ch, '█', "left channel row");
            assert_eq!(cell(&renderer, x, 6).ch, '█', "right channel row");
        }
        for x in 0..8 {
            assert_eq!(cell(&renderer, x, 2).ch, ' ');
        }
    }

    #[test]
    fn split_bars_use_each_channel_side_by_side() {
        let mut config = Config::default();
        config.general.bars = 2;
        config.general.bar_width = 1;
        config.general.channel_mode = ChannelMode::Split;
        let mut renderer = Renderer::with_size(&config, 8, 4).unwrap();
        renderer.paint_bars(&[1.0, 0.0], 0);
        renderer.paint_bars(&[1.0, 0.0], 1);

        for y in 0..4 {
            assert_eq!(cell(&renderer, 1, y).ch, '█', "left channel");
            assert_eq!(cell(&renderer, 5, y).ch, '█', "right channel");
            assert_eq!(cell(&renderer, 2, y).ch, ' ');
            assert_eq!(cell(&renderer, 6, y).ch, ' ');
        }
    }

    #[test]
    fn mirrored_bars_flip_the_second_channel() {
        let mut config = Config::default();
        config.general.bars = 2;
        config.general.bar_width = 1;
        config.general.channel_mode = ChannelMode::Mirror;
        let mut renderer = Renderer::with_size(&config, 8, 4).unwrap();
        renderer.paint_bars(&[1.0, 0.0], 0);
        renderer.paint_bars(&[1.0, 0.0], 1);

        // Group zero puts the loud band at column 1; the mirror puts it at 6.
        for y in 0..4 {
            assert_eq!(cell(&renderer, 1, y).ch, '█');
            assert_eq!(cell(&renderer, 6, y).ch, '█');
            assert_eq!(cell(&renderer, 5, y).ch, ' ');
        }
    }

    #[test]
    fn clearing_removes_a_previous_frame() {
        let mut renderer = test_renderer(4, 8);
        renderer.paint_bars(&[1.0, 1.0], 0);
        assert!(renderer.cells.iter().any(|c| c.ch != ' '));

        renderer.clear();
        assert!(renderer.cells.iter().all(|c| *c == Cell::default()));
    }
}
