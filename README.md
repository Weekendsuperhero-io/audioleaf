# Audioleaf

A TUI for managing and visualizing music on your Nanoleaf Canvas

![audioleaf tui view](https://github.com/alfazet/audioleaf/blob/main/images/tui.png)

## Installation

Install from cargo with `cargo install audioleaf`. Make sure that the directory with cargo binaries (by default `$HOME/.cargo/bin`) is added to your `$PATH`.

For users of Arch-based distros, audioleaf is also available as a [package in the AUR](https://aur.archlinux.org/packages/audioleaf). You can install it with your AUR helper of choice, for example with yay: `yay -S audioleaf`.

## Usage
At first launch `audioleaf` will locate Nanoleaf Canvas devices on your local network and prompt you to choose one of them to save as the default. All device data (names, local IPs and authentication tokens) is by default saved to `$HOME/.config/nl_devices.toml` (you can override this path with the `--devices` flag). When `audioleaf` is ran with no flags, it will try to connect to the first device on the list. 

If you'd like to add a new device later, you can do so with the `-n` (`--new`) option. If you want to specify which device to conenct to, use the `-d` (`--device-name`) option with the device's name (e.g. run `audioleaf -d "Canvas DD79"`).

Lost in the TUI? Press <kbd>?</kbd> to see the list of keybinds.

## Configuration
All configuration of audioleaf is done through the `config.toml` file, located in `$HOME/.config/audioleaf` (you can also override its location with the `--config` flag). At first launch, a default config file will be generated for you after establishing a connection to the Nanoleaf device. All the available options are described below:

### TUI configuration
* `colorful_effect_names`: Should the effect names in the TUI be colored according to their color palettes?

### Visualizer configuration
* `audio_backend`: The audio backend (e.g. pulseaudio) that will be the source of audio data for the visualizer.
* `freq_range`: The minimum and maximum frequencies (in Hz) to be included in the visualization.
* `hues`: A list of hues to be used in the visualizer's color palette, specified as angles between 0 and 360 degrees on the standard [color wheel](https://developer.mozilla.org/en-US/blog/learn-css-hues-colors-hsl/color-wheel.svg).
* `default_gain`: A non-negative real number, the bigger it is the more the audio samples are amplified before being visualized. While in audioleaf you can decrease and increase gain with the <kbd>-</kbd> and <kbd>+</kbd> keys. This settings doesn't affect your listening volume.
* `transition_time`: The time (in units of 100 ms) it will take for a panel to perform a full color change. 
* `time_window`: The length of the time window (in seconds) that audioleaf will collect audio samples from. A long time window can make the visualization look "jagged".
* `primary_axis`: The primary coordinate by which the panels will be sorted. Possible values are `"X"` (left → right) and `"Y"` (bottom → top).
* `sort_primary/secondary`: The direction in which the panels will be sorted on the primary/secondary axis. Possible values are `"Asc"` (ascending) and `"Desc"` (descending).

## Troubleshooting

Make sure that audioleaf's audio input is set to be your media player's output. On Linux this can be done with any audio mixer software, for example, with [pavucontrol](https://freedesktop.org/software/pulseaudio/pavucontrol) (for pulseaudio or pipewire), go to the *Recording* tab and make sure that the device in the dropdown menu is set correctly.

![pavucontrol](https://github.com/alfazet/audioleaf/blob/main/images/pavucontrol.png)

Windows doesn't have a "simple" way of re-routing one program's audio output to another's input. You'll have to use third-party software such as [VB Cable](https://vb-audio.com/Cable).

## Contributing

Audioleaf is a project made mainly in my spare time as a way to become familiar with Rust, making TUIs, and some basics of audio processing.

Therefore, there are surely many ways to make it more robust, performant and nicer to use - feel free to open a pull request or start a Github issue if you see any potential for audioleaf's improvement. Thank you!
