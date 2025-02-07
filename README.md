# Audioleaf

An audio visualizer for Nanoleaf Canvas

## Installation

Install from cargo with `cargo install audioleaf`. Make sure that the directory with cargo binaries (by default `$HOME/.cargo/bin`) is added to your `$PATH`.

For users of Arch-based distros, audioleaf is also available as a [package in the AUR](https://aur.archlinux.org/packages/audioleaf). You can install it with your AUR helper of choice, for example with yay: `yay -S audioleaf`.

## Configuration
All configuration of audioleaf is done through the `config.toml` file, located in `$HOME/.config/audioleaf`. At first launch a default config file will be generated for you. All the available options are described below:

* `ip`: The local IP address of the default Nanoleaf device audioleaf will connect to. 
* `port`: Port to be used for sending UDP data. 
* `use_colors`: Should the TUI display colorful output?
* `audio_device`: The audio input device that will be the source of audio data for the visualizer.
* `min/max_freq`: The minimum/maximum frequency (in Hz) to be included in the visualization.
* `default_gain`: A non-negative real number, the bigger it is the more the audio samples are amplified before being visualized. While in audioleaf you can decrease and increase gain with the <kbd>-</kbd> and <kbd>+</kbd> keys. This settings doesn't affect your listening volume.
* `transition_time`: The time (in units of 100 ms) it will take for a panel to perform a full color change. 
* `time_window`: The length of the time window that audioleaf will collect audio samples from. The longer it is, the smoother the visualization will be.
* `primary_axis`: The primary coordinate by which the panels will be sorted. Possible values are `"x"` (left → right) and `"y"` (bottom → top).
* `sort_primary/secondary`: The direction in which the panels will be sorted on the primary/secondary axis. Possible values are `"asc"` (ascending) and `"desc"` (descending).
* `hues`: A list of hues to be used in the visualizer's color palette, specified as angles between 0 and 360 degrees on the standard [color wheel](https://developer.mozilla.org/en-US/blog/learn-css-hues-colors-hsl/color-wheel.svg).
* `active_panels_numbers`: A list of numbers of panels that should be lit up during visualization. These numbers relate to the sorting method mentioned earlier. For example, if you sorted your panels first by Y ascending, then by X descending, then the first panel will be in the lower right-hand corner of your setup and the last one will be in the upper left-hand corner. Frequencies will be visualized according to these panel numbers: the higher the number, the higher the frequency. Note: these numbers *aren't* Nanoleaf's internal panel IDs.

## Troubleshooting

Make sure that audioleaf's audio input is set to be your media player's output. On Linux this can be done with any audio mixer software, for example [pavucontrol](https://freedesktop.org/software/pulseaudio/pavucontrol) (for pulseaudio or pipewire).

Windows doesn't have a default way to re-route one program's audio output as another's input. You'll have to use third-party software such as [VB Cable](https://vb-audio.com/Cable).
