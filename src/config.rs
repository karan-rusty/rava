//! User-facing configuration, optionally loaded from a TOML file.
//!
//! Every field has a default, so an empty file (or no `--config` at all)
//! yields the built-in look.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

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
pub struct General {
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
            bars: 0,
            bar_width: 2,
            bar_spacing: 0,
            channel_mode: ChannelMode::Average,
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
    /// Gradient stops, from the quietest bar to the loudest.
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
    pub general: General,
    pub output: Output,
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
