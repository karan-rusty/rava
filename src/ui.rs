//! The four visualizers and the timer overlay, ported from NoctaVox.

use std::time::Duration;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    text::Text,
    widgets::{
        Block, LineGauge, Padding, Widget,
        canvas::{Canvas, Context, Line, Rectangle},
    },
};

use crate::{
    theme::{Theme, fade_color, gradient_color, inactive_color},
    visualizer::{OSCILLO_LIMITER, ProgressDisplay, Visualizer},
};

const WAVEFORM_WIDGET_HEIGHT: f64 = 50.0;
const DEFAULT_AMP: f32 = 1.0;
const SPECTRUM_MAX_Y: f64 = 1.05;
const SPECTRUM_MIN_Y: f64 = 0.05;
const MIRROR_MIN_Y: f64 = -1.05;

/// Draw the active visualizer full-screen, with the timer and key hints on top.
pub fn draw(
    frame: &mut Frame,
    viz: &mut Visualizer,
    theme: &Theme,
    position: Duration,
    duration: Duration,
) {
    let area = frame.area();
    frame.render_widget(Block::default().bg(theme.bg), area);

    let elapsed = position.as_secs_f32();
    let played = progress(elapsed, duration.as_secs_f32());

    match viz.mode() {
        ProgressDisplay::Spectrum => {
            let bins = viz.spectrum_bins(area.width as usize);
            frame.render_widget(
                Spectrum {
                    bins,
                    theme,
                    elapsed,
                },
                area,
            );
        }
        // Until the waveform is decoded, the spectrum stands in for it.
        ProgressDisplay::Waveform => match viz.waveform_is_valid() {
            true => {
                let samples = viz.waveform();
                frame.render_widget(
                    Waveform {
                        samples,
                        theme,
                        elapsed,
                        played,
                    },
                    area,
                );
            }
            false => {
                let bins = viz.spectrum_bins(area.width as usize);
                frame.render_widget(
                    Spectrum {
                        bins,
                        theme,
                        elapsed,
                    },
                    area,
                );
            }
        },
        ProgressDisplay::Oscilloscope => {
            let samples = viz.display_tap();
            frame.render_widget(
                Oscilloscope {
                    samples,
                    theme,
                    elapsed,
                },
                area,
            );
        }
        ProgressDisplay::ProgressBar => {
            frame.render_widget(
                ProgressBar {
                    theme,
                    elapsed,
                    played,
                },
                area,
            );
        }
    }

    if area.height > 0 {
        timer(
            area,
            frame.buffer_mut(),
            theme,
            position.min(duration),
            duration,
            viz.mode(),
        );
        let hints = Rect::new(area.x, area.bottom() - 1, area.width, 1);
        let text = format!(
            " {} | w: next | space: pause | h/l or \u{2190}/\u{2192}: seek | {{/}}: smoothing {:.1} | q: quit ",
            viz.mode(),
            viz.smoothing()
        );
        frame.render_widget(Text::from(text).fg(theme.text_muted).bg(theme.bg), hints);
    }
}

/// Fraction played, clamped just short of 1 so the last cell stays unfinished.
fn progress(elapsed: f32, duration: f32) -> f32 {
    (elapsed / duration.max(f32::EPSILON)).clamp(0.0, 0.9999)
}

struct Spectrum<'a> {
    bins: &'a [f32],
    theme: &'a Theme,
    elapsed: f32,
}

impl Widget for Spectrum<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.bins.is_empty() {
            return;
        }

        // Braille packs 2 dots per cell, so the x axis is twice the width.
        let pixel_width = (area.width as usize * 2) as f64;
        let mirrored = self.theme.spectrum_mirror;
        let y_min = match mirrored {
            true => MIRROR_MIN_Y,
            false => SPECTRUM_MIN_Y,
        };

        Canvas::default()
            .x_bounds([0.0, pixel_width])
            .y_bounds([y_min, SPECTRUM_MAX_Y]) // the 0.05 keeps the bars off the edges
            .marker(self.theme.marker)
            .paint(|ctx| {
                for (i, &mag) in self.bins.iter().enumerate() {
                    let position = i as f32 / area.width.max(1) as f32;
                    let base = gradient_color(
                        self.theme.spectrum_colors,
                        position,
                        self.elapsed,
                        self.theme.spectrum_speed,
                    );
                    let color = fade_color(self.theme.dark, base, mag.max(0.35));

                    for x in [i * 2, i * 2 + 1] {
                        ctx.draw(&Line {
                            x1: x as f64,
                            y1: if mirrored { -(mag as f64) } else { 0.0 },
                            x2: x as f64,
                            y2: mag as f64,
                            color,
                        });
                    }
                }
            })
            .background_color(self.theme.bg)
            .block(Block::new().bg(self.theme.bg).padding(Padding {
                left: 10,
                right: 10,
                top: (area.height as f32 * 0.25) as u16,
                bottom: (area.height as f32 * 0.25) as u16,
            }))
            .render(area, buf)
    }
}

struct Waveform<'a> {
    samples: &'a [f32],
    theme: &'a Theme,
    elapsed: f32,
    played: f32,
}

impl Widget for Waveform<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let len = self.samples.len();
        if len == 0 {
            return;
        }

        let padding_vertical = match area.height {
            0..=6 => 0,
            7..=20 => (area.height as f32 * 0.15) as u16 - 1,
            21..=40 => (area.height as f32 * 0.25) as u16,
            _ => (area.height as f32 * 0.35) as u16,
        };

        Canvas::default()
            .x_bounds([0.0, len as f64])
            .y_bounds([-WAVEFORM_WIDGET_HEIGHT, WAVEFORM_WIDGET_HEIGHT])
            .marker(self.theme.marker)
            .paint(|ctx| {
                for (idx, amp) in self.samples.iter().enumerate() {
                    let hgt = (*amp as f64 * WAVEFORM_WIDGET_HEIGHT).round();
                    let position = idx as f32 / len as f32;

                    let color = match position < self.played {
                        true => gradient_color(
                            self.theme.waveform_colors,
                            position,
                            self.elapsed,
                            self.theme.waveform_speed,
                        ),
                        false => inactive_color(
                            self.theme.dark,
                            self.theme.waveform_colors,
                            position,
                            self.elapsed,
                            self.theme.waveform_speed,
                            *amp,
                        ),
                    };

                    // Lines read better in small windows, rectangles in a
                    // full-screen one.
                    match area.width < 170 {
                        true => draw_waveform_line(ctx, idx as f64, hgt, color),
                        false => draw_waveform_rect(ctx, idx as f64, hgt, color),
                    }
                }
            })
            .background_color(self.theme.bg)
            .block(Block::new().bg(self.theme.bg).padding(Padding {
                left: 10,
                right: 10,
                top: padding_vertical + 1,
                bottom: padding_vertical + 1,
            }))
            .render(area, buf)
    }
}

fn draw_waveform_line(ctx: &mut Context, idx: f64, hgt: f64, color: ratatui::style::Color) {
    ctx.draw(&Line {
        x1: idx,
        x2: idx,
        y1: hgt,
        y2: -hgt,
        color,
    });
}

fn draw_waveform_rect(ctx: &mut Context, idx: f64, hgt: f64, color: ratatui::style::Color) {
    ctx.draw(&Rectangle {
        x: idx,
        y: -hgt,
        width: 0.5,        // keeps the waveform clean on resize
        height: hgt * 2.0, // rectangles grow from the bottom
        color,
    });
}

struct Oscilloscope<'a> {
    samples: &'a [f32],
    theme: &'a Theme,
    elapsed: f32,
}

impl Widget for Oscilloscope<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let n = OSCILLO_LIMITER.min(self.samples.len());
        let samples = &self.samples[self.samples.len() - n..];
        if samples.is_empty() {
            return;
        }

        let v_marg = match area.height > 20 {
            true => ((area.height as f32) * 0.25) as u16,
            false => 0,
        };

        Canvas::default()
            .x_bounds([0.0, samples.len() as f64])
            .y_bounds([-1.0, 1.0])
            .marker(self.theme.marker)
            .paint(|ctx| {
                let peak = samples
                    .iter()
                    .map(|s| s.abs())
                    .max_by(|a, b| a.total_cmp(b))
                    .unwrap_or(1.0);
                let scale = if peak > 1.0 { 1.0 / peak } else { 1.0 };

                for (i, window) in samples.windows(2).enumerate() {
                    let progress = i as f32 / samples.len() as f32;
                    // Slowed right down: the older NoctaVox gradient scroll.
                    let time = self.elapsed / 4.0;
                    let color = gradient_color(
                        self.theme.oscilloscope_colors,
                        progress,
                        time,
                        self.theme.oscilloscope_speed,
                    );

                    ctx.draw(&Line {
                        x1: i as f64,
                        y1: (window[0] * scale) as f64,
                        x2: (i + 1) as f64,
                        y2: (window[1] * scale) as f64,
                        color,
                    });
                }
            })
            .background_color(self.theme.bg)
            .block(Block::new().bg(self.theme.bg).padding(Padding {
                left: 1,
                right: 1,
                top: v_marg,
                bottom: v_marg,
            }))
            .render(area, buf)
    }
}

struct ProgressBar<'a> {
    theme: &'a Theme,
    elapsed: f32,
    played: f32,
}

impl Widget for ProgressBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let pos = if self.theme.progress_speed == 0.0 {
            0.0
        } else {
            self.played
        };

        let fg = gradient_color(
            self.theme.progress_colors,
            pos,
            self.elapsed,
            self.theme.progress_speed,
        );
        let bg = inactive_color(
            self.theme.dark,
            self.theme.progress_colors,
            self.played,
            self.elapsed,
            self.theme.progress_speed,
            DEFAULT_AMP,
        );

        let x_pad = 9;
        let y_pad = area.height.saturating_sub(1) / 2;

        LineGauge::default()
            .block(Block::new().bg(self.theme.bg).padding(Padding {
                left: x_pad,
                right: x_pad,
                top: y_pad,
                bottom: 0,
            }))
            .filled_style(fg)
            .unfilled_style(bg)
            .filled_symbol(self.theme.progress_played)
            .unfilled_symbol(self.theme.progress_unplayed)
            .label("")
            .ratio(self.played as f64)
            .render(area, buf)
    }
}

fn timer(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    elapsed: Duration,
    duration: Duration,
    mode: ProgressDisplay,
) {
    let y_pos = match mode {
        ProgressDisplay::Oscilloscope => area.y + 1,
        _ => area.y + (area.height.saturating_sub(1) / 2),
    };

    let elapsed_str = compact(elapsed);
    let duration_str = compact(duration);

    let x_elapsed = 2 + (8_u16.saturating_sub(elapsed_str.len() as u16) / 2);
    let x_duration = (area.x + area.width.saturating_sub(10))
        + (10_u16.saturating_sub(duration_str.len() as u16) / 2);

    let width = elapsed_str.len() as u16;
    Text::from(elapsed_str)
        .fg(theme.text_muted)
        .render(Rect::new(x_elapsed, y_pos, width, 1), buf);

    let width = duration_str.len() as u16;
    Text::from(duration_str)
        .fg(theme.text_muted)
        .render(Rect::new(x_duration, y_pos, width, 1), buf);
}

/// `m:ss` (or `h:mm:ss`), NoctaVox's `DurationStyle::Compact`.
fn compact(duration: Duration) -> String {
    let mut secs = duration.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    secs %= 60;

    match hours {
        0 => format!("{mins}:{secs:02}"),
        _ => format!("{hours}:{mins:02}:{secs:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_clamped() {
        assert_eq!(progress(0.0, 90.0), 0.0);
        assert_eq!(progress(45.0, 90.0), 0.5);
        assert!(progress(90.0, 90.0) < 1.0);
        assert_eq!(progress(0.0, 0.0), 0.0); // before the duration is known
    }

    #[test]
    fn compact_matches_noctavox() {
        assert_eq!(compact(Duration::from_secs(9)), "0:09");
        assert_eq!(compact(Duration::from_secs(605)), "10:05");
        assert_eq!(compact(Duration::from_secs(3661)), "1:01:01");
    }
}
