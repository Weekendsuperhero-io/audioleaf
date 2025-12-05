# Changelog

All notable changes to this fork of audioleaf are documented in this file.

This fork focuses on macOS compatibility, support for all Nanoleaf device types, and enhanced color palette features.

## [3.2.0] - 2025-12-05

### Added

- **Named color palette support**: Easy-to-use preset color palettes
  - Use `hues = "palette-name"` instead of manually specifying hue arrays
  - 11 built-in palettes: ocean-nightclub, sunset, house-music-party, tropical-beach, fire, forest, neon-rainbow, pink-dreams, cool-blues, tmnt, christmas
  - Still supports custom hue arrays: `hues = [0, 60, 120, 180, 240, 300]`
  - Error messages show available palette names if invalid name is used
  - Location: `src/palettes.rs` (new module)

- **White color support**: Special hue value for white colors
  - Use `360` in hue arrays to create white/near-white colors
  - Example: `hues = [0, 120, 360]` creates red, green, and white
  - Internally sets high whiteness value in HWB color space
  - Perfect for Christmas theme and other palettes needing white accents
  - Location: `src/utils.rs:colors_from_hues()`, `src/config.rs`

- **Multi-device type support**: Added SSDP discovery for all Nanoleaf device types
  - Canvas (nl29)
  - Shapes (nl42)
  - Elements (nl52)
  - Light Panels (nanoleaf_aurora:light)
  - Previous version only supported Canvas devices

- **SSDP deduplication**: Prevent duplicate device entries when multiple SSDP responses are received for the same device

- **macOS config directory support**: Properly handle macOS Application Support directory
  - Config location: `~/Library/Application Support/audioleaf/`
  - Device data location: `~/Library/Application Support/audioleaf/`
  - Previous version assumed Linux-style `~/.config/` on all platforms

- **Flexible config parsing**: Accept both integer and float values for `default_gain` in config.toml
  - Previous version only accepted float values
  - TOML parsers naturally parse `500` as integer, not `500.0`

- **Comprehensive documentation**:
  - Complete configuration reference with all options explained
  - Platform-specific audio setup guides (macOS, Linux, Windows)
  - Pre-made color palette examples using HSB color system
  - HSB color wheel reference (0-359 hue values)
  - Detailed troubleshooting section
  - Brightness adjustment instructions
  - Clarified that Nanoleaf API uses HSB (Hue, Saturation, Brightness) color space

### Fixed

- **Panel filtering**: Exclude controller units from visualization panel list
  - Controller panels have `shapeType >= 12` and should not be included in the visualization
  - Fixes issue where only one panel would light up (the controller unit)
  - Affected Shapes, Elements, and other devices with separate controller units
  - Location: `src/nanoleaf.rs:get_panels()`

- **Directory creation**: Automatically create config and data directories if they don't exist
  - Fixes "No such file or directory (os error 2)" errors on first run
  - Applies to both config files and device data files
  - Locations: `src/config.rs:write_to_file()`, `src/nanoleaf.rs:append_to_file()`

- **Enter key handling**: Fix TUI effect selection with Enter key
  - Removed duplicate event handling that caused double-triggering
  - Now only processes Press events, ignoring Release events
  - Tested on Ghostty, iTerm2, and Alacritty terminal emulators
  - Location: `src/event_handler.rs`

- **Key event handling in prompts**: Improved `wait_for_any_key()` using crossterm raw mode
  - Properly detects key presses in terminal prompts
  - Uses crossterm event polling instead of raw stdin reading
  - Location: `src/utils.rs:wait_for_any_key()`

### Changed

- **SSDP discovery loop**: Modified discovery to search for multiple device types sequentially
  - Sends separate M-SEARCH requests for each device type
  - Collects responses with timeout handling between searches
  - Location: `src/ssdp.rs`

- **Default color palette**: Updated to "Ocean Nightclub" theme
  - Hues: `[195, 210, 240, 270, 285, 300, 180]`
  - Blues, purples, and teals for a nightclub ambiance
  - Previous default was more generic rainbow palette

- **Recommended gain values**: Updated documentation for audio devices
  - macOS BlackHole 2ch (targeted directly): `default_gain = 1`
  - Windows VB Cable: `default_gain = 200-500` (may vary)
  - Physical microphones: `default_gain = 1-10`
  - Important: Target BlackHole 2ch directly, not the Multi-Output aggregate device

## Technical Details

### SSDP Service Types

The following SSDP service types are now supported:

| Service Type            | Device Model          | Notes              |
| ----------------------- | --------------------- | ------------------ |
| `nanoleaf:nl29`         | Canvas                | Original support   |
| `nanoleaf:nl42`         | Shapes                | Added in this fork |
| `nanoleaf:nl52`         | Elements              | Added in this fork |
| `nanoleaf_aurora:light` | Light Panels (Aurora) | Added in this fork |

### Panel Shape Types

Panel filtering logic excludes controller units:

| Shape Type | Description      | Include in Visualization |
| ---------- | ---------------- | ------------------------ |
| 0-11       | Light panels     | ✓ Yes                    |
| 12+        | Controller units | ✗ No (filtered out)      |

### Platform-Specific Paths

| Platform | Config Path                                           | Device Data Path                                          |
| -------- | ----------------------------------------------------- | --------------------------------------------------------- |
| macOS    | `~/Library/Application Support/audioleaf/config.toml` | `~/Library/Application Support/audioleaf/nl_devices.toml` |
| Linux    | `~/.config/audioleaf/config.toml`                     | `~/.config/audioleaf/nl_devices.toml`                     |
| Windows  | `%APPDATA%\audioleaf\config.toml`                     | `%APPDATA%\audioleaf\nl_devices.toml`                     |

### Audio Gain Values

Recommended gain settings based on audio source:

| Audio Source                  | Configuration                     | Recommended Gain |
| ----------------------------- | --------------------------------- | ---------------- |
| BlackHole 2ch (macOS, direct) | `audio_backend = "BlackHole 2ch"` | 1                |
| VB Cable (Windows)            | Varies by setup                   | 200-500          |
| Physical microphone           | Direct input                      | 1-10             |

**Important for macOS users**: Set `audio_backend` to `"BlackHole 2ch"` (the virtual device itself), not the Multi-Output Device aggregate. This provides proper audio levels with `default_gain = 1`.

The gain value amplifies the FFT output before visualization, not the audio playback volume.

## Upstream

This fork is based on [audioleaf](https://github.com/alfazet/audioleaf) by alfazet.

Original project features:

- TUI for Nanoleaf control
- Audio visualization using FFT
- Customizable color palettes
- Panel sorting and layout options

## Credits

- Original author: [alfazet](https://github.com/alfazet)
- macOS compatibility and Shapes support: This fork
