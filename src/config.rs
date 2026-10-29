//! User-facing configuration, optionally loaded from a TOML file.
//!
//! Every field has a default, so an empty file (or no `--config` at all)
//! yields the built-in look.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::spectrum::DEFAULT_DECAY;

/// Which visualizer starts up, and what `w` cycles through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// NoctaVox's auto-gained braille spectrum.
    #[default]
    Spectrum,
    /// Wide eighth-block bars with a vertical gradient.
    Bars,
}

impl Mode {
    /// The next mode in the cycle used by the `w` key.
    pub fn next(self) -> Self {
        match self {
            Self::Spectrum => Self::Bars,
            Self::Bars => Self::Spectrum,
        }
    }
}

/// How the two audio channels are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChannelMode {
    /// Both channels folded into a single bar per band.
    #[default]
    Average,
    /// Left channel and right channel side by side.
    Split,
    /// The left channel and its mirror image.
    Mirror,
}

/// Which way the bars grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

/// How bar colors are picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    /// Interpolate across `output.gradient`.
    #[default]
    Gradient,
    /// Use the single `output.color` everywhere.
    Solid,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Input {
    /// `"auto"` monitors the default sink; anything else is a PulseAudio or
    /// PipeWire source name.
    pub source: String,
    /// RMS level (dBFS) at or below which audio counts as silence.
    pub threshold_db: f32,
    /// Milliseconds of silence before the screen goes blank.
    pub idle_after_ms: u64,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            source: "auto".to_owned(),
            threshold_db: -60.0,
            idle_after_ms: 1_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    /// Visualizer shown at startup; `w` switches between the two at runtime.
    pub mode: Mode,
    /// Number of bars to draw; `0` means as many as fit the terminal.
    pub bars: usize,
    /// Columns (or rows, when horizontal) reserved for each bar.
    pub bar_width: u16,
    /// Blank columns between neighboring bars.
    pub bar_spacing: u16,
    pub channel_mode: ChannelMode,
}

impl Default for General {
    fn default() -> Self {
        Self {
            mode: Mode::Spectrum,
            bars: 0,
            bar_width: 2,
            bar_spacing: 0,
            channel_mode: ChannelMode::Average,
        }
    }
}

/// Settings for the NoctaVox-style spectrum. `[spectrum]` ignores the bar
/// layout in `[general]`: it always draws one braille column per band.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpectrumSettings {
    /// Grow the bars both above and below the center line.
    pub mirror: bool,
    /// How fast bars fall after a peak, between `0.7` and `0.97`; higher is
    /// slower. Out-of-range values are clamped by the analyser.
    pub decay: f32,
    /// Gradient scroll speed in cycles per second; `0.0` keeps it still.
    pub speed: f32,
}

impl Default for SpectrumSettings {
    fn default() -> Self {
        Self {
            mirror: false,
            decay: DEFAULT_DECAY,
            speed: 0.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub orientation: Orientation,
    pub color_mode: ColorMode,
    /// Color used when `color_mode = "solid"`.
    pub color: String,
    /// Gradient stops: bottom to top on the bars, left to right on the spectrum.
    pub gradient: Vec<String>,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            orientation: Orientation::Vertical,
            color_mode: ColorMode::Gradient,
            color: "#00e5ff".to_owned(),
            gradient: ["#00e5ff", "#2979ff", "#7c4dff", "#ff4081"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub input: Input,
    pub general: General,
    pub output: Output,
    pub spectrum: SpectrumSettings,
}

impl Config {
    /// Load `path`, or return the defaults when no path was given.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read config `{}`", path.display()))?;
        Self::parse(&text).with_context(|| format!("invalid config `{}`", path.display()))
    }

    /// Parse TOML into a configuration.
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("could not parse TOML")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_the_builtin_look() {
        let config = Config::parse("").unwrap();
        assert_eq!(config.input.source, "auto");
        assert_eq!(config.input.threshold_db, -60.0);
        assert_eq!(config.input.idle_after_ms, 1_000);
        assert_eq!(config.general.mode, Mode::Spectrum);
        assert_eq!(config.general.bars, 0);
        assert!(!config.spectrum.mirror);
        assert_eq!(config.spectrum.decay, DEFAULT_DECAY);
        assert_eq!(config.spectrum.speed, 0.0);
    }

    #[test]
    fn parses_live_input_settings() {
        let config = Config::parse(
            r#"
            [input]
            source = "alsa_output.usb.monitor"
            threshold_db = -45.0
            idle_after_ms = 250
            "#,
        )
        .unwrap();

        assert_eq!(config.input.source, "alsa_output.usb.monitor");
        assert_eq!(config.input.threshold_db, -45.0);
        assert_eq!(config.input.idle_after_ms, 250);
    }

    #[test]
    fn parses_mode_and_spectrum_overrides() {
        let config = Config::parse(
            r#"
            [general]
            mode = "bars"

            [spectrum]
            mirror = true
            decay = 0.9
            speed = 0.25
            "#,
        )
        .unwrap();

        assert_eq!(config.general.mode, Mode::Bars);
        assert!(config.spectrum.mirror);
        assert_eq!(config.spectrum.decay, 0.9);
        assert_eq!(config.spectrum.speed, 0.25);
    }

    #[test]
    fn shipped_config_parses() {
        let config = Config::parse(include_str!("../assets/config.toml")).unwrap();
        assert_eq!(config.input.source, "auto");
        assert_eq!(config.input.threshold_db, -60.0);
        assert_eq!(config.general.mode, Mode::Spectrum);
        assert_eq!(config.output.gradient.len(), 4);
        assert_eq!(config.spectrum.decay, DEFAULT_DECAY);
    }

    #[test]
    fn rejects_unknown_keys_and_modes() {
        assert!(Config::parse("[spectrum]\nunknown = 1\n").is_err());
        assert!(Config::parse("[general]\nmode = \"waveform\"\n").is_err());
    }

    #[test]
    fn modes_cycle_back_to_where_they_started() {
        assert_eq!(Mode::Spectrum.next().next(), Mode::Spectrum);
        assert_eq!(Mode::Bars.next(), Mode::Spectrum);
    }
}
