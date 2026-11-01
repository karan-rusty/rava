# rava

A terminal audio visualizer for Linux, written in Rust. It reads what your system is playing and draws it in the terminal. With live input the screen stays blank while nothing plays.

![The spectrum view](docs/rava-spectrum.png)

Press `w` to switch between the mirrored braille spectrum, adapted from [NoctaVox](https://github.com/Jaxx497/NoctaVox), and wide eighth-block bars.

## Install

Prebuilt binary:

```sh
curl -LsSf https://raw.githubusercontent.com/karan-rusty/rava/main/install.sh | sh
```

From source:

```sh
cargo install --git https://github.com/karan-rusty/rava --locked
```

The prebuilt binaries are Linux only. Live capture needs `parec` from PulseAudio or PipeWire. `--file` and `--demo` do not. Building from source needs Rust 1.95 or newer.

## Run

```sh
rava                       # live system audio
rava --source NAME         # a specific monitor or source
rava --file track.wav      # loop a WAV
rava --demo                # built-in synth loop
rava --mode bars           # start on the bars
```

`w` switches views. `q`, `Esc`, or `Ctrl+C` quits.

Live capture uses the monitor of the default sink. For a different source, take a name from `pactl list short sources`; monitor sources end in `.monitor`.

## Configure

rava takes an optional TOML file:

```sh
rava --config assets/config.toml
```

Every key is optional. `assets/config.toml` documents them all. Unknown keys are errors.

## Credits

The spectrum and braille rendering are adapted from [NoctaVox](https://github.com/Jaxx497/NoctaVox) by Jaxx497, under the MIT license. See `THIRD_PARTY_NOTICES.md`. rava is MIT licensed.
