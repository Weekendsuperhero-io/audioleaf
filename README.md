# Audioleaf

A real-time music visualizer for Nanoleaf devices (Shapes, Canvas, Elements, and Light Panels). Audioleaf listens to your system audio, analyzes it, and drives your Nanoleaf panels with reactive color animations — all rendered in a graphical window that mirrors your physical panel layout.

![Audioleaf GUI](Assets/gui.png)

![Audioleaf Demo](Assets/demo.gif)

> **Note:** This is a fork with macOS compatibility fixes, a graphical UI, album art integration, and support for all Nanoleaf device types. See [CHANGELOG.md](CHANGELOG.md) for details.

## Features

- **Real-time audio visualization** — Three effects (Spectrum, Energy Wave, Pulse) that react to your music
- **Graphical panel preview** — See your exact Nanoleaf layout rendered on screen with live color preview
- **Album art integration** — Automatically extract color palettes from the currently playing track's album artwork (Spotify & Apple Music)
- **11 built-in color palettes** — From ocean-nightclub to neon-rainbow, plus custom RGB palettes
- **Panel sorting controls** — Adjust how colors map to your physical layout
- **Cross-platform** — macOS and Linux

## Installation

Install from cargo:

```bash
cargo install audioleaf
```

Make sure `$HOME/.cargo/bin` is in your `$PATH`.

For Arch-based distros, audioleaf is also available in the [AUR](https://aur.archlinux.org/packages/audioleaf):

```bash
yay -S audioleaf
```

## Usage

### First-Time Setup

At first launch, audioleaf discovers Nanoleaf devices on your local network:

```bash
audioleaf -n
```

This will:

1. Scan your network for Nanoleaf devices
2. Display discovered devices
3. Prompt you to put your device in pairing mode (hold power button until LEDs flash)
4. Save the device configuration

**Device data location:**

- **macOS**: `~/Library/Application Support/audioleaf/nl_devices.toml`
- **Linux**: `~/.config/audioleaf/nl_devices.toml`
- **Custom**: Use `--devices /path/to/devices.toml`

### Running

After setup, simply run:

```bash
audioleaf
```

To connect to a specific device:

```bash
audioleaf -d "Shapes AC01"
```

### Controls

Press <kbd>?</kbd> in the app to see all keybinds.

| Key | Action |
| --- | --- |
| <kbd>Esc</kbd> / <kbd>Q</kbd> | Quit |
| <kbd>?</kbd> | Toggle help overlay |
| <kbd>Space</kbd> | Toggle live panel color preview |
| <kbd>-</kbd> / <kbd>+</kbd> | Decrease / increase gain (visual sensitivity) |
| <kbd>1</kbd>-<kbd>9</kbd>, <kbd>0</kbd> | Switch color palette |
| <kbd>E</kbd> | Cycle effect: Spectrum / Energy Wave / Pulse |
| <kbd>A</kbd> | Toggle primary sort axis (X / Y) |
| <kbd>P</kbd> | Toggle primary sort (Asc / Desc) |
| <kbd>S</kbd> | Toggle secondary sort (Asc / Desc) |
| <kbd>N</kbd> | Use album art colors from current track |
| <kbd>R</kbd> | Reset all panels to black |

### Effects

- **Spectrum** — Each panel tracks a frequency band. Bass on one end, treble on the other.
- **Energy Wave** — Audio energy cascades across panels as a traveling ripple.
- **Pulse** — All panels pulse together, driven by audio transients. Snaps to the beat.

## Configuration

Configuration lives in `config.toml`:

- **macOS**: `~/Library/Application Support/audioleaf/config.toml`
- **Linux**: `~/.config/audioleaf/config.toml`
- **Custom**: Use `--config /path/to/config.toml`

A default config file is generated on first launch.

### Example Configuration

```toml
default_nl_device_name = "Shapes AC01"

[visualizer_config]
# Audio input device (see Audio Setup below)
audio_backend = "BlackHole 2ch"

# Frequency range to visualize [min_hz, max_hz]
freq_range = [20, 4500]

# Color palette — named palette or custom RGB array
# Named: "ocean-nightclub", "sunset", "fire", "forest", "neon-rainbow", etc.
colors = "ocean-nightclub"
# Or custom RGB: colors = [[255, 0, 128], [0, 128, 255], [128, 255, 0]]

# Audio sensitivity (doesn't affect playback volume)
default_gain = 1.0

# Panel transition speed in 100ms units (2 = 200ms)
transition_time = 2

# Audio sampling window in seconds
time_window = 0.1875

# Panel sorting
primary_axis = "Y"        # "X" or "Y"
sort_primary = "Asc"      # "Asc" or "Desc"
sort_secondary = "Asc"    # "Asc" or "Desc"

# Visualization effect
effect = "Spectrum"        # "Spectrum", "EnergyWave", or "Pulse"
```

### Available Palettes

| Palette | Description |
| --- | --- |
| `ocean-nightclub` | Deep blues, purples, teals |
| `sunset` | Warm oranges, reds, pinks |
| `house-music-party` | Energetic magentas, purples, cyans |
| `tropical-beach` | Turquoise, aqua, lime |
| `fire` | Reds, oranges, yellows |
| `forest` | Deep greens, yellow-green |
| `neon-rainbow` | Full spectrum |
| `pink-dreams` | Soft pinks through magentas |
| `cool-blues` | Ice blues to navy |
| `tmnt` | Turtle green + bandana colors |
| `christmas` | Red, green, white |

## Audio Setup

### macOS

1. Install [BlackHole](https://existential.audio/blackhole/) (free virtual audio device)
2. Open **Audio MIDI Setup** (Applications > Utilities)
3. Create a **Multi-Output Device** including your speakers + BlackHole 2ch
4. Set the Multi-Output Device as your system output
5. Set `audio_backend = "BlackHole 2ch"` in config.toml

**Tip**: Target `"BlackHole 2ch"` directly, not the Multi-Output Device aggregate. This provides proper audio levels with `default_gain = 1`.

### Linux (PulseAudio/PipeWire)

1. Run audioleaf
2. Open `pavucontrol` (PulseAudio Volume Control)
3. In the **Recording** tab, set audioleaf's input to your media player's monitor
4. Set `audio_backend` in config.toml to match

## Dump Commands

Inspect your device without launching the full app:

```bash
# Show panel layout info
audioleaf dump layout

# Interactive graphical layout view (click panels to flash them)
audioleaf dump layout-graphical

# List available color palettes
audioleaf dump palettes

# Show raw device info
audioleaf dump info
```

## Troubleshooting

### Visualizer Not Responding

1. **Check audio routing** — Verify audioleaf receives audio input (use `pavucontrol` on Linux, check Multi-Output Device on macOS)
2. **Adjust gain** — Press <kbd>+</kbd>/<kbd>-</kbd> in the app, or set `default_gain` in config
3. **Adjust frequency range** — Try `freq_range = [20, 500]` for bass-heavy, `[20, 4500]` for full range

### Device Not Discovered

1. Ensure device is powered on and on the same network
2. Check firewall isn't blocking SSDP/UDP multicast
3. Try `audioleaf -n` to re-discover

### Brightness

Brightness is controlled by your Nanoleaf device settings (mobile app or physical buttons), not by audioleaf. The visualizer dynamically adjusts color intensity based on audio.

## Contributing

Feel free to open a pull request or start a GitHub issue. Contributions welcome!
