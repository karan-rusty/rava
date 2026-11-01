# rava

rava is a terminal audio visualizer for Linux, written in Rust. It draws live
system audio as a mirrored braille spectrum or wide bars, and stays blank while
nothing plays.

![The spectrum view](docs/rava-spectrum.png)

Press `w` to switch between the braille spectrum and the wide bars.

## Install

```sh
cargo install --git https://github.com/karan-rusty/rava --locked
```

Needs Rust 1.95 or newer. Live audio also needs `parec` (PulseAudio or PipeWire).

Reinstall with `--force` to upgrade; `cargo uninstall rava` removes it.

## Use

```sh
rava                       # live system audio
rava --source NAME         # a specific source
rava --file track.wav      # loop a WAV
rava --demo                # built-in loop
rava --mode bars           # start on the bars
```

`w` switches views. `q`, `Esc` or `Ctrl+C` quits.

## Configure

```sh
rava --config assets/config.toml
```

Every key is optional, and `assets/config.toml` documents them all.

## Credits

The spectrum is adapted from [NoctaVox](https://github.com/Jaxx497/NoctaVox) (MIT). rava is MIT licensed.
