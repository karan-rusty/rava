//! Colors and gradients, ported from NoctaVox's default (dark) display theme.

use ratatui::{style::Color, symbols::Marker};

/// NoctaVox sharpens the ramp interpolation; same curve here so the ramps match.
const SHARP_FACTOR: f32 = 2.0;

const DARK_WHITE: Color = Color::Rgb(210, 210, 213);
const MID_GRAY: Color = Color::Rgb(100, 100, 103);
const DARK_GRAY: Color = Color::Rgb(25, 25, 28);
const DARK_GRAY_FADED: Color = Color::Rgb(15, 15, 18);
const GOOD_RED_DARK: Color = Color::Rgb(180, 30, 30);

const NONWAVEFORM_SPEED: f32 = 0.0;
const WAVEFORM_SPEED: f32 = 4.0;

const DEFAULT_RAMP: &[Color] = &[DARK_WHITE, GOOD_RED_DARK, DARK_GRAY];

const BAR_SYMBOL_PLAYED: &str = "━";
const BAR_SYMBOL_UNPLAYED: &str = "─";

/// Scrolls a color ramp by `position + time * speed`; NoctaVox's ramp curve,
/// with the sharpened interpolation so the stops read as stops.
pub fn gradient_color(ramp: &[Color], position: f32, time: f32, speed: f32) -> Color {
    let t = ((position + time * speed) % 1.0).abs();

    if ramp.is_empty() {
        return Color::Reset;
    }

    let segment_count = ramp.len();
    let segment_f = t * segment_count as f32;
    let segment = (segment_f as usize).min(segment_count - 1);
    let local_t = segment_f - segment as f32;

    let next_segment = (segment + 1) % segment_count;
    let sharp = sharpen_interpolation(local_t, SHARP_FACTOR);

    interpolate_color(ramp[segment], ramp[next_segment], sharp)
}

pub struct Theme {
    pub dark: bool,
    pub bg: Color,
    pub text_muted: Color,
    pub marker: Marker,

    pub spectrum_colors: &'static [Color],
    pub spectrum_mirror: bool,
    /// Per-frame multiplier for bars on their way down (NoctaVox default 0.85).
    pub spectrum_decay: f32,
    pub spectrum_speed: f32,

    pub oscilloscope_colors: &'static [Color],
    pub oscilloscope_speed: f32,

    pub waveform_colors: &'static [Color],
    pub waveform_speed: f32,

    pub progress_colors: &'static [Color],
    pub progress_speed: f32,
    pub progress_played: &'static str,
    pub progress_unplayed: &'static str,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            dark: true,
            bg: DARK_GRAY_FADED,
            text_muted: MID_GRAY,
            marker: Marker::Braille,

            spectrum_colors: DEFAULT_RAMP,
            spectrum_mirror: false,
            spectrum_decay: 0.85,
            spectrum_speed: NONWAVEFORM_SPEED,

            oscilloscope_colors: DEFAULT_RAMP,
            oscilloscope_speed: NONWAVEFORM_SPEED,

            waveform_colors: DEFAULT_RAMP,
            waveform_speed: WAVEFORM_SPEED / 10.0,

            progress_colors: DEFAULT_RAMP,
            progress_speed: NONWAVEFORM_SPEED,
            progress_played: BAR_SYMBOL_PLAYED,
            progress_unplayed: BAR_SYMBOL_UNPLAYED,
        }
    }
}

/// Color for the "not played yet" side of the waveform and the progress bar:
/// the played ramp dimmed, brightness following the sample amplitude.
pub fn inactive_color(
    dark: bool,
    played: &[Color],
    position: f32,
    time: f32,
    speed: f32,
    amp: f32,
) -> Color {
    let brightness = match played.len() {
        0 | 1 => 0.4,
        _ => 0.12 + (amp * 0.4),
    };
    fade_color(
        dark,
        gradient_color(played, position, time, speed),
        brightness,
    )
}

pub fn fade_color(is_dark: bool, color: Color, factor: f32) -> Color {
    match is_dark {
        true => dim_color(color, factor),
        false => brighten_color(color, factor),
    }
}

fn interpolate_color(c1: Color, c2: Color, t: f32) -> Color {
    match (c1, c2) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => Color::Rgb(
            (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8,
            (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8,
            (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8,
        ),
        _ => c1,
    }
}

fn sharpen_interpolation(t: f32, power: f32) -> f32 {
    if t < 0.5 {
        (t * 2.0).powf(power) / 2.0
    } else {
        1.0 - ((1.0 - t) * 2.0).powf(power) / 2.0
    }
}

fn dim_color(color: Color, factor: f32) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as f32 * factor) as u8,
            (g as f32 * factor) as u8,
            (b as f32 * factor) as u8,
        ),
        other => other,
    }
}

fn brighten_color(color: Color, factor: f32) -> Color {
    let factor = 1.0 - factor;
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as f32 + (255.0 - r as f32) * factor) as u8,
            (g as f32 + (255.0 - g as f32) * factor) as u8,
            (b as f32 + (255.0 - b as f32) * factor) as u8,
        ),
        other => other,
    }
}
