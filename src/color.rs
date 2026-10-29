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

    /// Dim the color toward black, as NoctaVox does for quiet spectrum bars.
    pub fn scale(self, factor: f32) -> Self {
        let apply = |channel: u8| (channel as f32 * factor).clamp(0.0, 255.0) as u8;
        Self::new(apply(self.r), apply(self.g), apply(self.b))
    }

    /// Appends the ANSI 24-bit foreground parameters (without the `ESC[`/`m`).
    pub fn write_fg(self, out: &mut String) {
        use std::fmt::Write;
        let _ = write!(out, "38;2;{};{};{}", self.r, self.g, self.b);
    }

    /// Appends the ANSI 24-bit background parameters (without the `ESC[`/`m`).
    pub fn write_bg(self, out: &mut String) {
        use std::fmt::Write;
        let _ = write!(out, "48;2;{};{};{}", self.r, self.g, self.b);
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

    /// NoctaVox's scrolling ramp.
    ///
    /// The position (plus `time * speed`) wraps around the ends, and each
    /// segment is sharpened so gradient stops read as stops instead of a
    /// continuous smear. A one-stop gradient is a solid color.
    pub fn at_wrapped(&self, position: f32, time: f32, speed: f32) -> Rgb {
        if self.stops.len() == 1 {
            return self.stops[0];
        }

        let t = (position + time * speed).rem_euclid(1.0);
        let count = self.stops.len();
        let segment_f = t * count as f32;
        let segment = (segment_f as usize).min(count - 1);
        let local = segment_f - segment as f32;
        let sharp = sharpen(local);
        self.stops[segment].lerp(self.stops[(segment + 1) % count], sharp)
    }
}

/// Ease each gradient segment so the stops dominate the middle of the ramp.
fn sharpen(t: f32) -> f32 {
    if t < 0.5 {
        (t * 2.0).powi(2) / 2.0
    } else {
        1.0 - ((1.0 - t) * 2.0).powi(2) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_dims_every_channel() {
        assert_eq!(Rgb::new(200, 100, 10).scale(0.5), Rgb::new(100, 50, 5));
        assert_eq!(Rgb::new(200, 100, 10).scale(2.0), Rgb::new(255, 200, 20));
    }

    #[test]
    fn wrapped_gradient_cycles_back_to_the_first_stop() {
        let red = Rgb::new(255, 0, 0);
        let blue = Rgb::new(0, 0, 255);
        let gradient = Gradient::new(vec![red, blue]).unwrap();

        assert_eq!(gradient.at_wrapped(0.0, 0.0, 0.0), red);
        assert_eq!(gradient.at_wrapped(0.5, 0.0, 0.0), blue);
        // One full cycle lands on the same color again.
        assert_eq!(gradient.at_wrapped(0.0, 1.0, 1.0), red);
        assert_eq!(
            gradient.at_wrapped(0.9, 0.2, 1.0),
            gradient.at_wrapped(0.1, 0.0, 0.0)
        );
    }

    #[test]
    fn static_gradient_ignores_position_and_time() {
        let color = Rgb::new(1, 2, 3);
        let gradient = Gradient::new(vec![color]).unwrap();
        assert_eq!(gradient.at_wrapped(0.4, 12.0, 3.0), color);
    }
}
