# Audioleaf

A TUI for managing and visualizing music on your Nanoleaf devices (Canvas, Shapes, Elements, and Light Panels)

![audioleaf tui view](https://github.com/alfazet/audioleaf/blob/main/images/tui.png)

> **Note:** This is a fork with macOS compatibility fixes and support for all Nanoleaf device types. See [CHANGELOG.md](CHANGELOG.md) for details.

## Installation

Install from cargo with `cargo install audioleaf`. Make sure that the directory with cargo binaries (by default `$HOME/.cargo/bin`) is added to your `$PATH`.

For users of Arch-based distros, audioleaf is also available as a [package in the AUR](https://aur.archlinux.org/packages/audioleaf). You can install it with your AUR helper of choice, for example with yay: `yay -S audioleaf`.

## Usage

### First-Time Setup

At first launch, `audioleaf` will automatically discover Nanoleaf devices on your local network and prompt you to choose one:

```bash
audioleaf -n
```

This will:

1. Scan your network for Nanoleaf devices (Canvas, Shapes, Elements, Light Panels)
2. Display a list of discovered devices
3. Prompt you to put your chosen device in pairing mode (hold power button until LEDs flash)
4. Save the device configuration

**Device data location:**

- **macOS**: `~/Library/Application Support/audioleaf/nl_devices.toml`
- **Linux**: `~/.config/audioleaf/nl_devices.toml`
- **Custom**: Use `--devices /path/to/devices.toml` to specify a custom location

### Running Audioleaf

After initial setup, simply run:

```bash
audioleaf
```

To connect to a specific device:

```bash
audioleaf -d "Shapes AC01"
```

### TUI Controls

Lost in the TUI? Press <kbd>?</kbd> to see the list of keybinds.

**Main shortcuts:**

- <kbd>Enter</kbd> - Play selected effect
- <kbd>V</kbd> - Toggle visualizer mode
- <kbd>j/k</kbd> or <kbd>↓/↑</kbd> - Navigate effect list
- <kbd>+/-</kbd> - Increase/decrease visualizer gain (sensitivity)
- <kbd>Q</kbd> or <kbd>Esc</kbd> - Quit
- <kbd>?</kbd> - Show help

## Configuration

All configuration is done through the `config.toml` file. The location depends on your operating system:

- **macOS**: `~/Library/Application Support/audioleaf/config.toml`
- **Linux**: `~/.config/audioleaf/config.toml`
- **Custom**: Use `--config /path/to/config.toml` to override

At first launch, a default config file will be automatically generated after connecting to your Nanoleaf device.

### Complete Configuration Reference

```toml
# Specify which Nanoleaf device to use by default
default_nl_device_name = "Shapes AC01"

[tui_config]
# Display effect names in colors matching their palettes
colorful_effect_names = true

[visualizer_config]
# Audio input device name (see Audio Setup section below)
audio_backend = "BlackHole 2ch"

# Frequency range to visualize (Hz) - [min, max]
# Lower range = bass-focused, higher range = treble-focused
freq_range = [20, 4500]

# Color palette as hue values (0-360 degrees on HSV color wheel)
# See Color Palettes section for examples
hues = [195, 210, 240, 270, 285, 300, 180]

# Audio sensitivity multiplier (default: 1.0)
# BlackHole 2ch directly: 1 (recommended)
# VB Cable or other virtual devices: may need 200-500
# Physical microphones: typically 1-10
default_gain = 1

# Panel transition speed in 100ms units (1 = 100ms, 10 = 1s)
transition_time = 2

# Audio sampling window in seconds
# Smaller = more responsive, larger = smoother but laggy
time_window = 0.1875

# Panel layout sorting (affects visualization direction)
primary_axis = "Y"        # "X" (left→right) or "Y" (bottom→top)
sort_primary = "Asc"      # "Asc" or "Desc"
sort_secondary = "Asc"    # "Asc" or "Desc"
```

### Configuration Options Explained

#### TUI Configuration

| Option                  | Type    | Default | Description                                              |
| ----------------------- | ------- | ------- | -------------------------------------------------------- |
| `colorful_effect_names` | boolean | `true`  | Display effect names with colors matching their palettes |

#### Visualizer Configuration

| Option            | Type       | Default         | Description                                                     |
| ----------------- | ---------- | --------------- | --------------------------------------------------------------- |
| `audio_backend`   | string     | (auto-detected) | Name of audio input device to visualize                         |
| `freq_range`      | [int, int] | `[20, 4500]`    | Min and max frequencies (Hz) to include in visualization        |
| `hues`            | [int, ...] | varies          | List of HSV hue values (0-360°) for color palette               |
| `default_gain`    | int/float  | `1.0`           | Audio amplification multiplier (doesn't affect playback volume) |
| `transition_time` | int        | `2`             | Panel color transition time in 100ms units                      |
| `time_window`     | float      | `0.1875`        | Audio sampling window length in seconds                         |
| `primary_axis`    | string     | `"Y"`           | Primary sort direction: `"X"` or `"Y"`                          |
| `sort_primary`    | string     | `"Asc"`         | Primary sort order: `"Asc"` or `"Desc"`                         |
| `sort_secondary`  | string     | `"Asc"`         | Secondary sort order: `"Asc"` or `"Desc"`                       |

### Color Palettes

Colors are specified using HSV hue values (0-360 degrees). Here are some pre-made palettes:

**Ocean Nightclub** (blues, purples, teals)

```toml
hues = [195, 210, 240, 270, 285, 300, 180]
```

**Sunset** (oranges, reds, purples)

```toml
hues = [0, 15, 30, 330, 300, 270]
```

**Neon** (bright rainbow)

```toml
hues = [0, 60, 120, 180, 240, 300]
```

**Forest** (greens and yellows)

```toml
hues = [60, 80, 100, 120, 140]
```

**Fire** (reds and yellows)

```toml
hues = [0, 15, 30, 45, 60]
```

Use a [color wheel reference](https://developer.mozilla.org/en-US/blog/learn-css-hues-colors-hsl/color-wheel.svg) to pick custom hues:

- Red: 0°
- Orange: 30°
- Yellow: 60°
- Green: 120°
- Cyan: 180°
- Blue: 240°
- Purple: 270°
- Magenta: 300°

### Audio Setup

#### macOS

1. Install [BlackHole](https://existential.audio/blackhole/) (free virtual audio device)
2. Open **Audio MIDI Setup** (Applications → Utilities)
3. Create a **Multi-Output Device**:
   - Include your speakers/headphones
   - Include BlackHole 2ch
4. Set the Multi-Output Device as your system output
5. Set `audio_backend = "BlackHole 2ch"` in config.toml
6. Set `default_gain = 1` (normal gain when targeting BlackHole directly)

**Important**: Target `"BlackHole 2ch"` directly, not the Multi-Output Device aggregate. This provides proper audio levels without requiring extreme gain values.

#### Linux (PulseAudio/PipeWire)

1. Run audioleaf once to see available devices
2. Use `pavucontrol` (PulseAudio Volume Control)
3. Go to **Recording** tab while audioleaf is running
4. Set audioleaf's input to your media player's monitor
5. Set `audio_backend` to match the device name
6. Adjust `default_gain` as needed (typically 1-10)

#### Windows

1. Install [VB Cable](https://vb-audio.com/Cable) (free virtual audio cable)
2. Set VB Cable as your default playback device
3. Route VB Cable output to your speakers using audio software
4. Set `audio_backend = "VB Cable"` in config.toml
5. Set `default_gain = 200` or higher

### Brightness Adjustment

Brightness is controlled by your Nanoleaf device settings, not by audioleaf:

1. Open the Nanoleaf mobile app
2. Select your device
3. Adjust the brightness slider
4. The visualizer will maintain this brightness level

Alternatively, you can adjust brightness directly on the Nanoleaf controller using the physical buttons.

## Troubleshooting

### Visualizer Not Responding to Music

**Symptom**: Panels don't react to audio, or react very weakly.

**Solutions**:

1. **Check audio routing**: Ensure audioleaf is receiving audio input
   - On Linux: Use `pavucontrol` → Recording tab to verify audio source
   - On macOS: Verify Multi-Output Device is selected in System Settings
   - On Windows: Confirm VB Cable routing is correct

2. **Adjust gain**: Different audio sources need different gain levels
   - Press <kbd>+</kbd>/<kbd>-</kbd> in audioleaf to adjust gain in real-time
   - BlackHole 2ch (macOS): `default_gain = 1` when targeted directly
   - VB Cable (Windows): may need `default_gain = 200-500`
   - Physical microphones: typically `default_gain = 1-10`

3. **Adjust frequency range**: Try different frequency ranges
   - Bass-heavy: `freq_range = [20, 500]`
   - Full range: `freq_range = [20, 4500]`
   - Treble-focused: `freq_range = [1000, 8000]`

### Only One Panel Lights Up

**Symptom**: Only a single panel shows colors during visualization.

**Solution**: This is fixed in this fork. The original version included the controller unit in the panel list. If you're still seeing this:

- Update to the latest version of this fork
- Run `audioleaf -n` to re-discover your device
- Delete the devices file and reconnect

### Device Not Discovered

**Symptom**: "No Nanoleaf devices found on the local network"

**Solutions**:

1. Ensure your device is powered on and connected to the same network
2. This fork supports all device types: Canvas (nl29), Shapes (nl42), Elements (nl52), and Light Panels (aurora)
3. Check your firewall isn't blocking SSDP/UDP multicast
4. Try running with `sudo` on Linux if permission issues exist
5. Manually specify device in config: `default_nl_device_name = "Your Device Name"`

### Enter Key Not Working in TUI

**Symptom**: Pressing Enter doesn't select effects.

**Solution**: This is fixed in this fork. If still experiencing issues:

- Ensure you're running the latest version
- Try different terminal emulators (tested with Ghostty, iTerm2, Alacritty)
- Check terminal supports standard key event handling

### Config Changes Not Taking Effect

**Symptom**: Changing config.toml has no effect.

**Solutions**:

1. **Verify correct config location**:
   - macOS: `~/Library/Application Support/audioleaf/config.toml`
   - Linux: `~/.config/audioleaf/config.toml`
   - Not in `~/.config/` on macOS!

2. **Check TOML syntax**: Ensure proper formatting
   - Use integers OR floats for `default_gain` (both work)
   - Arrays use square brackets: `hues = [0, 60, 120]`
   - Strings use quotes: `audio_backend = "BlackHole 2ch"`

3. **Restart audioleaf**: Changes only apply on launch

### Audio Routing (Linux)

Make sure that audioleaf's audio input is set to be your media player's output. Use any audio mixer software, for example [pavucontrol](https://freedesktop.org/software/pulseaudio/pavucontrol) (for PulseAudio or PipeWire):

1. Go to the **Recording** tab while audioleaf is running
2. Set the device in the dropdown menu to your media player's monitor

![pavucontrol](https://github.com/alfazet/audioleaf/blob/main/images/pavucontrol.png)

### Audio Routing (Windows)

Windows doesn't have a built-in way to route one program's audio output to another's input. Use third-party software such as [VB Cable](https://vb-audio.com/Cable).

## Contributing

Audioleaf is a project made mainly in my spare time as a way to become familiar with Rust, making TUIs, and some basics of audio processing.

Therefore, there are surely many ways to make it more robust, performant and nicer to use - feel free to open a pull request or start a Github issue if you see any potential for audioleaf's improvement. Thank you!
