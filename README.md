# rava — a terminal audio visualizer

A terminal audio visualizer written in Rust. Its spectrum is a port of the one
in [NoctaVox](https://github.com/Jaxx497/NoctaVox); the original bar renderer is
still available too.

## Run

```sh
cargo run --release
cargo run --release -- --file path/to/track.wav
cargo run --release -- --mode bars
cargo run --release -- --config assets/config.toml
```

Without `--file` rava synthesizes a short beat loop, so it runs anywhere.
`--file` decodes a WAV (any sample rate or channel count) and loops it.

Keys:

| Key | Action |
|---|---|
| `w` | switch between the spectrum and the bars |
| `q`, `Esc` | quit |

## Visualizers

### `spectrum` (default)

NoctaVox's spectrum: a 2048-point Hann-windowed FFT over the last mono samples,
folded into roughly 100 bands from 20 Hz to 20 kHz. Each band is auto-gained
against its own running peak — instant attack, slow release — and falls by an
exponential decay factor, then the bands are interpolated to one braille column
per terminal column. Color comes from a horizontal gradient that dims toward
black as the band gets quieter. Optional: `mirror = true` grows the bars above
and below the center line, and `speed` scrolls the gradient.

![The spectrum view in a 120x40 terminal](docs/spectrum.png)

### `bars`

Wide eighth-block bars, one per band, with a vertical gradient over the bar
height. Layout is controlled by `[general]` (`bars`, `bar_width`,
`bar_spacing`, `channel_mode`, `orientation`).

Both views use the same audio and `[output]` colors; `w` switches instantly.

## Configuration

Every key is optional; an empty file gives the built-in look. Unknown keys and
unknown enum values are errors.

```toml
[general]
# Visualizer to start with: "spectrum" or "bars" (`w` toggles at runtime).
mode = "spectrum"
# Bars only: 0 means "as many as fit".
bars = 0
bar_width = 2
bar_spacing = 0
# "average", "split", or "mirror".
channel_mode = "average"

[output]
orientation = "vertical"        # or "horizontal" (bars only)
color_mode = "gradient"         # or "solid" to use `color`
color = "#00e5ff"
gradient = ["#00e5ff", "#2979ff", "#7c4dff", "#ff4081"]

[spectrum]
mirror = false
# How fast bars fall after a peak (0.7–0.97; higher is slower).
decay = 0.85
# Gradient scroll in cycles per second; 0 keeps it still.
speed = 0.0
```

`assets/config.toml` contains the same values with comments.

## Credits

The spectrum analyser and braille rendering are adapted from
[NoctaVox](https://github.com/Jaxx497/NoctaVox) by Jaxx497 (MIT). See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
