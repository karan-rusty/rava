<div align="center">
<h1>rava
</h1>

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust: 1.95+](https://img.shields.io/badge/Rust-1.95%2B-000?logo=rust&logoColor=fff&labelColor=444&color=fff)](https://www.rust-lang.org)
[![Built With: crossterm](https://img.shields.io/badge/Built_With-crossterm-000?labelColor=444&color=fff)](https://github.com/crossterm-rs/crossterm)

**A lightweight, Rust-powered terminal audio visualizer for Linux that draws
what your system is playing.**
</div>

![rava-spectrum.png](./docs/rava-spectrum.png)

## Features

- NoctaVox-style mirrored braille spectrum with a true 20 Hz..20 kHz log axis
- Wide eighth-block bars, vertical or horizontal
- Average, split or mirrored stereo for the bars
- Live capture of the default output through its PulseAudio/PipeWire monitor
- Blank screen while nothing plays; wakes on the first audible chunk
- WAV playback with low-passed resampling to the output rate
- Built-in synth loop, no audio server required
- TOML configuration with warm gradient or solid colors
- Runtime resize, and `w` to switch visualizers

## Installation

#### Cargo *(recommended)*
```bash
cargo install --git https://github.com/karan-rusty/rava --locked
```

The binary lands in `~/.cargo/bin`. To upgrade, rerun the command with
`--force`; to remove it, `cargo uninstall rava`.

#### Build Git Version

```bash
git clone https://github.com/karan-rusty/rava.git
cd rava

# Run directly (use --release for a smooth framerate)
cargo run --release -- --demo

# Or install globally
cargo install --path . --locked
```

> **Building on Linux** \
> Needs Rust 1.95 or newer; there are no system libraries to install. Live
> capture shells out to `parec`, which ships with PulseAudio
> (`pulseaudio-utils`) and with PipeWire's PulseAudio compatibility layer.
> `--file` and `--demo` do not need it.

## Quick Start

**Switch visualizers:** `w`  
**Quit:** `q`, `Esc` or `Ctrl`+`c`

```bash
rava                       # live system audio
rava --source NAME         # a specific monitor or source
rava --file track.wav      # loop a WAV
rava --demo                # built-in synth loop
rava --mode bars           # start on the bars
```

`auto` capture uses the monitor of the default sink. For a different source,
take a name from `pactl list short sources`; monitor sources end in `.monitor`.

## Silence

With live input the screen starts blank. The first audible chunk wakes it
immediately, and it goes blank again once the input has been quieter than
`-60 dBFS` RMS for one second. Both values live in `[input]`: set
`idle_after_ms = 0` to blank on the first silent chunk, or lower
`threshold_db` so very quiet audio counts as playing. Nothing is drawn while
idle, so a blank screen with the terminal still captured is expected.

## Configuration

rava takes an optional TOML file; every key is optional and the values below
are the built-in defaults, so the file mostly serves as documentation.

```bash
rava --config assets/config.toml
```

```toml
[input]
source = "auto"          # default sink monitor, or a name from `pactl list short sources`
threshold_db = -60.0     # RMS at or below this counts as silence
idle_after_ms = 1000     # silence before the screen blanks; 0 blanks immediately

[general]
mode = "spectrum"        # "spectrum" or "bars"
bars = 0                 # bars per channel group; 0 = as many as fit
bar_width = 2            # columns (or rows, when horizontal) per bar
bar_spacing = 0          # blank columns (or rows) between bars
channel_mode = "average" # "average", "split" or "mirror" (bars only)

[output]
orientation = "vertical" # or "horizontal" (bars only)
color_mode = "gradient"  # or "solid" to use `color` everywhere
color = "#d2a55c"
gradient = ["#5d7372", "#a8794e", "#e0b56b", "#f2dcb4"] # quietest to loudest

[spectrum]
mirror = true            # false grows the bars from the bottom
decay = 0.85             # 0.7..=0.97; higher holds peaks longer
speed = 0.0              # gradient scroll in cycles per second
```

> **Note:** Unknown keys are rejected rather than ignored, and a parse error
> stops rava instead of silently falling back to defaults. `assets/config.toml`
> contains the same values with longer comments.

## FAQ

#### Nothing is drawn even though audio is playing.

Live capture needs `parec` and a source that is actually playing. Check both
with `pactl list short sources` and make sure the chosen source is a monitor of
the sink you hear. Very quiet audio can fall under `threshold_db`; lower it.

#### Where do I get a source name?

`pactl list short sources` lists them. Sources ending in `.monitor` carry what
an output device is playing; the second column is the name to pass to
`--source`. Leaving it at `auto` picks the monitor of the default sink.

#### Does rava play or record audio?

No. It only reads the monitor mix and draws it, and it never writes to audio
files or devices.

#### Why is `--file` limited to WAV?

`hound` decodes PCM WAV, mono or stereo at any sample rate, which rava then
resamples to 44.1 kHz. Convert anything else first, for example
`ffmpeg -i track.mp3 track.wav`.

## Other

rava is a hobby project and a small demonstration of DSP in Rust: one FFT
pipeline shared by two renderers, braille rasterisation, and 24-bit terminal
colour. It is MIT licensed and keeps no state on disk.

## Special Thanks

- [NoctaVox](https://github.com/Jaxx497/NoctaVox) by Jaxx497 — the spectrum
  analyser and braille rendering are adapted from it under the MIT license; see
  [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)
- [crossterm](https://github.com/crossterm-rs/crossterm) — terminal backend
- [realfft](https://github.com/HEnquist/realfft) — FFTs
- [hound](https://github.com/ruuda/hound) — WAV decoding
- [clap](https://github.com/clap-rs/clap), [serde](https://serde.rs) and
  [toml](https://github.com/toml-rs/toml) — CLI and config
