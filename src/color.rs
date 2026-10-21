use anyhow::{Result, bail};

/// An 8-bit sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    #[allow(dead_code)]
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim().to_ascii_lowercase();
        if s.is_empty() {
            bail!("empty color");
        }
        if let Some(hex) = s.strip_prefix('#').or_else(|| s.strip_prefix("0x")) {
            return parse_hex(hex);
        }
        let rgb = match s.as_str() {
            "black" => (0x00, 0x00, 0x00),
            "red" => (0xff, 0x00, 0x00),
            "green" => (0x00, 0xff, 0x00),
            "yellow" => (0xff, 0xff, 0x00),
            "blue" => (0x00, 0x00, 0xff),
            "magenta" | "fuchsia" => (0xff, 0x00, 0xff),
            "cyan" | "aqua" => (0x00, 0xff, 0xff),
            "white" => (0xff, 0xff, 0xff),
            "silver" => (0xc0, 0xc0, 0xc0),
            "gray" | "grey" => (0x80, 0x80, 0x80),
            "darkgray" | "darkgrey" => (0x40, 0x40, 0x40),
            "orange" => (0xff, 0x98, 0x00),
            "pink" => (0xff, 0x40, 0x81),
            "purple" => (0x9c, 0x27, 0xb0),
            "violet" => (0x7c, 0x4d, 0xff),
            "teal" => (0x00, 0x96, 0x88),
            "lime" => (0xcd, 0xdc, 0x39),
            "navy" => (0x00, 0x00, 0x80),
            "maroon" => (0x80, 0x00, 0x00),
            "olive" => (0x80, 0x80, 0x00),
            "brown" => (0x8b, 0x45, 0x13),
            _ => bail!("unknown color `{input}`"),
        };
        Ok(Self::new(rgb.0, rgb.1, rgb.2))
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self::new(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }

    /// Appends the ANSI 24-bit foreground parameters (without the `ESC[`/`m`).
    pub fn write_fg(self, out: &mut String) {
        use std::fmt::Write;
        let _ = write!(out, "38;2;{};{};{}", self.r, self.g, self.b);
    }
}

fn parse_hex(hex: &str) -> Result<Rgb> {
    let nibble = |c: u8| -> Result<u8> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            _ => bail!("invalid hex digit `{}`", c as char),
        }
    };
    let bytes = hex.as_bytes();
    let component = |i: usize| -> Result<u8> { Ok(nibble(bytes[i])? * 16 + nibble(bytes[i + 1])?) };
    match bytes.len() {
        3 => Ok(Rgb::new(
            nibble(bytes[0])? * 17,
            nibble(bytes[1])? * 17,
            nibble(bytes[2])? * 17,
        )),
        6 => Ok(Rgb::new(component(0)?, component(2)?, component(4)?)),
        _ => bail!("expected 3 or 6 hex digits, got `{hex}`"),
    }
}

/// A multi-stop color gradient.
#[derive(Debug, Clone)]
pub struct Gradient {
    stops: Vec<Rgb>,
}

impl Gradient {
    pub fn new(stops: Vec<Rgb>) -> Result<Self> {
        if stops.is_empty() {
            bail!("gradient needs at least one color");
        }
        Ok(Self { stops })
    }

    /// Return the color at position `t` in `0.0..=1.0`.
    pub fn at(&self, t: f32) -> Rgb {
        if self.stops.len() == 1 {
            return self.stops[0];
        }
        let t = t.clamp(0.0, 1.0) * (self.stops.len() - 1) as f32;
        let index = (t.floor() as usize).min(self.stops.len() - 2);
        let local = t - index as f32;
        self.stops[index].lerp(self.stops[index + 1], local)
    }
}
