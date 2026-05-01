use crate::{constants, ssdp, utils};
use anyhow::{Result, bail};
use serde::Serialize;
use std::fs::File;
use std::{
    io::prelude::*,
    net::Ipv4Addr,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

#[derive(Copy, Clone, Debug, Default, Serialize)]
pub enum Axis {
    X,
    #[default]
    Y,
}

#[derive(Copy, Clone, Debug, Default, Serialize)]
pub enum Sort {
    #[default]
    Asc,
    Desc,
}

/// Visual effect mode for the audio visualizer.
///
/// Controls how audio data maps to panel brightness/color animation.
#[derive(Copy, Clone, Debug, Default, Serialize)]
pub enum Effect {
    /// Each panel independently tracks a logarithmic frequency band.
    /// Brightness pulses with audio energy in that band (fast attack, slow decay).
    #[default]
    Spectrum,
    /// Each panel tracks its own frequency band (like Spectrum) but brightness
    /// bleeds into neighboring panels, creating a flowing wave across the array.
    EnergyWave,
    /// Onset-triggered pulses propagate outward from panel 0 like ripples on water
    /// or a starship jumping to warp — bright leading edge stretching into a fading trail.
    Ripple,
}

/// Where the visualizer pulls its panel colors from.
///
/// `Palette { name }` looks up the named effect on the active Nanoleaf device
/// and uses its palette. `name = None` means "use the device's currently-
/// selected effect." `Artwork` drives colors from album cover art when audio
/// is playing, falling back to a static dim white when idle.
#[derive(Copy, Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColorSourceKind {
    #[default]
    Palette,
    Artwork,
}

#[derive(Clone, Debug, Serialize)]
pub struct VisualizerConfig {
    pub audio_backend: Option<String>,
    pub freq_range: Option<(u16, u16)>,
    pub color_source: Option<ColorSourceKind>,
    /// Name of a Nanoleaf-side effect whose palette we use. `None` (with
    /// `color_source = Palette`) means "use the device's currently-selected
    /// effect." Ignored when `color_source = Artwork`.
    pub palette_name: Option<String>,
    pub default_gain: Option<f32>,
    pub transition_time: Option<u16>,
    pub time_window: Option<f32>,
    pub primary_axis: Option<Axis>,
    pub sort_primary: Option<Sort>,
    pub sort_secondary: Option<Sort>,
    pub effect: Option<Effect>,
}

impl Default for VisualizerConfig {
    fn default() -> Self {
        VisualizerConfig {
            audio_backend: Some("default".to_string()),
            freq_range: Some(constants::DEFAULT_FREQ_RANGE),
            // Defaults: pull palette from whatever effect the device is
            // currently set to. Users override via the API.
            color_source: Some(ColorSourceKind::Palette),
            palette_name: None,
            default_gain: Some(constants::DEFAULT_GAIN),
            transition_time: Some(constants::DEFAULT_TRANSITION_TIME),
            time_window: Some(constants::DEFAULT_TIME_WINDOW),
            primary_axis: Some(Axis::default()),
            sort_primary: Some(Sort::default()),
            sort_secondary: Some(Sort::default()),
            effect: Some(Effect::default()),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    pub default_nl_device_name: Option<String>,
    pub visualizer_config: VisualizerConfig,
}

impl Config {
    /// Constructs a new `Config` instance with optional component overrides.
    ///
    /// Uses defaults for unspecified sub-configs via their `default()` methods.
    ///
    /// # Arguments
    ///
    /// * `default_nl_device_name` - Optional default Nanoleaf device name for quick selection.
    /// * `visualizer_config` - Optional visualizer params; defaults to `VisualizerConfig::default()`.
    pub fn new(
        default_nl_device_name: Option<String>,
        visualizer_config: Option<VisualizerConfig>,
    ) -> Self {
        Config {
            default_nl_device_name,
            visualizer_config: visualizer_config.unwrap_or_default(),
        }
    }

    /// Parses a TOML table into the fields of a mutable `VisualizerConfig`.
    ///
    /// Supports comprehensive field validation and type conversion:
    /// - `audio_backend`: String for device name.
    /// - `freq_range`: 2-element array of u16 [min_hz, max_hz].
    /// - `colors`: Array of [R,G,B] triplets or string name of predefined palette (e.g., "ocean-nightclub").
    /// - `default_gain`: f32 or i64, applied to spectrum amplitudes.
    /// - `transition_time`: u16 in 100ms units for Nanoleaf transitions (0 = instant).
    /// - `time_window`: f32 seconds for smoothing window.
    /// - `primary_axis`: "X" or "Y" enum.
    /// - `sort_primary`/`sort_secondary`: "Asc" or "Desc".
    ///
    /// Validates ranges (e.g., RGB 0-255, transition_time >= 0) and bails on errors or unknown keys.
    /// Palette names checked against available predefined palettes.
    ///
    /// # Arguments
    ///
    /// * `visualizer_config` - Mutable reference to update.
    /// * `t` - TOML table from [visualizer_config] section.
    ///
    /// # Errors
    ///
    /// `anyhow::Error` for parsing failures, invalid values, or unknown keys.
    pub fn parse_visualizer_config(
        visualizer_config: &mut VisualizerConfig,
        t: toml::Table,
    ) -> Result<()> {
        for (key, val) in t {
            match (key.as_str(), val) {
                ("audio_backend", Value::String(s)) => {
                    visualizer_config.audio_backend = Some(s);
                }
                ("freq_range", Value::Array(v)) => {
                    if v.len() != 2 {
                        bail!("freq_range must be a 2-element integer array");
                    }
                    let (Some(low), Some(high)) = (v[0].as_integer(), v[1].as_integer()) else {
                        bail!("freq_range must be a 2-element integer array");
                    };
                    visualizer_config.freq_range =
                        Some((u16::try_from(low)?, u16::try_from(high)?));
                }
                ("color_source", Value::String(s)) => {
                    let kind = match s.to_ascii_lowercase().as_str() {
                        "palette" => ColorSourceKind::Palette,
                        "artwork" => ColorSourceKind::Artwork,
                        _ => bail!("color_source must be `palette` or `artwork`, got `{}`", s),
                    };
                    visualizer_config.color_source = Some(kind);
                }
                ("palette_name", Value::String(s)) => {
                    visualizer_config.palette_name = Some(s);
                }
                // Legacy keys removed by migrate_obsolete_fields() before parse.
                // If they survive (e.g. external write), silently drop them
                // rather than failing — we don't want a stale field to brick
                // startup.
                ("colors" | "hues", _) => {}
                ("default_gain", Value::Float(x)) => {
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Parsed default_gain as Float: {}", x);
                    visualizer_config.default_gain = Some(x as f32);
                }
                ("default_gain", Value::Integer(x)) => {
                    #[cfg(debug_assertions)]
                    eprintln!("DEBUG: Parsed default_gain as Integer: {}", x);
                    visualizer_config.default_gain = Some(x as f32);
                }
                ("transition_time", Value::Integer(x)) => {
                    let trans_time = u16::try_from(x).map_err(|_| {
                        anyhow::anyhow!(
                            "transition_time must be 0-65535. Note: units are in 100ms (0 = instant, 1 = 100ms, 2 = 200ms, etc.)"
                        )
                    })?;
                    visualizer_config.transition_time = Some(trans_time);
                }
                ("time_window", Value::Float(x)) => {
                    visualizer_config.time_window = Some(x as f32);
                }
                ("primary_axis", Value::String(s)) => {
                    let axis = match s.as_str() {
                        "X" => Some(Axis::X),
                        "Y" => Some(Axis::Y),
                        _ => None,
                    };
                    if axis.is_none() {
                        bail!("axis must be `X` or `Y`");
                    };
                    visualizer_config.primary_axis = axis;
                }
                ("sort_primary", Value::String(s)) => {
                    let sort = match s.as_str() {
                        "Asc" => Some(Sort::Asc),
                        "Desc" => Some(Sort::Desc),
                        _ => None,
                    };
                    if sort.is_none() {
                        bail!("sort must be `Asc` (ascending) or `Desc` (descending)");
                    };
                    visualizer_config.sort_primary = sort;
                }
                ("sort_secondary", Value::String(s)) => {
                    let sort = match s.as_str() {
                        "Asc" => Some(Sort::Asc),
                        "Desc" => Some(Sort::Desc),
                        _ => None,
                    };
                    if sort.is_none() {
                        bail!("sort must be `Asc` (ascending) or `Desc` (descending)");
                    };
                    visualizer_config.sort_secondary = sort;
                }
                ("effect", Value::String(s)) => {
                    let effect = match s.as_str() {
                        "Spectrum" | "spectrum" => Some(Effect::Spectrum),
                        "EnergyWave" | "energy_wave" | "energy-wave" => Some(Effect::EnergyWave),
                        "Ripple" | "ripple" => Some(Effect::Ripple),
                        _ => None,
                    };
                    if effect.is_none() {
                        bail!(
                            "effect must be `Spectrum`, `EnergyWave`, or `Ripple`, got `{}`",
                            s
                        );
                    };
                    visualizer_config.effect = effect;
                }
                (key, _) => {
                    bail!(format!("invalid key `{}`", key));
                }
            }
        }
        Ok(())
    }

    /// Loads and parses the full application configuration from a TOML file.
    ///
    /// Reads the file content, deserializes to TOML `Table`, then:
    /// - Extracts optional `default_nl_device_name` string.
    /// - Parses `[tui_config]` section using `parse_tui_config`.
    /// - Parses `[visualizer_config]` section using `parse_visualizer_config`.
    /// - Uses defaults for missing sections or fields.
    /// - Bails on unknown top-level keys or sub-config parse errors.
    ///
    /// Debug-logs file path and contents for verification.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the config.toml file.
    ///
    /// # Returns
    ///
    /// `Result<Config>` - Fully parsed and validated configuration.
    ///
    /// # Errors
    ///
    /// File I/O errors, TOML deserialization failures, or validation bails.
    ///
    /// One-shot migration: if the on-disk config still has obsolete `colors`
    /// or `hues` fields under `[visualizer_config]`, strip them and rewrite
    /// the file. Uses `toml_edit` so comments, key order, and whitespace
    /// outside the removed lines are preserved. No-op if neither key is
    /// present.
    fn migrate_obsolete_fields(path: &Path) -> Result<()> {
        let mut contents = String::new();
        File::open(path)?.read_to_string(&mut contents)?;

        let mut doc: toml_edit::DocumentMut = match contents.parse() {
            Ok(d) => d,
            // If the file isn't valid TOML, let the main parser produce a
            // proper error — we don't want to mangle a half-broken file.
            Err(_) => return Ok(()),
        };

        let mut removed = Vec::new();
        if let Some(item) = doc.get_mut("visualizer_config")
            && let Some(table) = item.as_table_mut()
        {
            for legacy in ["colors", "hues"] {
                if table.remove(legacy).is_some() {
                    removed.push(legacy);
                }
            }
        }

        if !removed.is_empty() {
            let new_contents = doc.to_string();
            std::fs::write(path, new_contents)?;
            eprintln!(
                "INFO: migrated {} — removed obsolete inline {}; palettes now come from the Nanoleaf device.",
                path.display(),
                removed.join(" and ")
            );
        }
        Ok(())
    }

    pub fn parse_from_file(path: &Path) -> Result<Self> {
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Reading config from: {}", path.display());
        // Strip obsolete fields from the on-disk file before parse. After this
        // returns, the file is guaranteed not to contain `colors` / `hues` in
        // [visualizer_config], regardless of what was there before.
        Self::migrate_obsolete_fields(path)?;

        let mut config_file = File::open(path)?;
        let mut contents = String::new();
        config_file.read_to_string(&mut contents)?;
        #[cfg(debug_assertions)]
        eprintln!("DEBUG: Config file contents:\n{}", contents);
        let data = contents.parse::<Table>()?;

        let mut default_nl_device_name = None;
        let mut visualizer_config = VisualizerConfig::default();
        for (key, val) in data {
            match (key.as_str(), val) {
                ("default_nl_device_name", Value::String(s)) => {
                    default_nl_device_name = Some(s);
                }
                // Silently ignore legacy tui_config section for backwards compatibility
                ("tui_config", Value::Table(_)) => {}
                ("visualizer_config", Value::Table(t)) => {
                    Self::parse_visualizer_config(&mut visualizer_config, t)?;
                }
                (key, _) => {
                    bail!(format!("invalid key `{}`", key));
                }
            }
        }
        Ok(Config::new(default_nl_device_name, Some(visualizer_config)))
    }

    /// Serializes and writes the configuration to a TOML file at the given path.
    ///
    /// Uses `toml::to_string_pretty` for readable formatting.
    /// Automatically creates parent directories if they do not exist.
    ///
    /// # Arguments
    ///
    /// * `self` - The config to serialize.
    /// * `path` - Target file path for config.toml.
    ///
    /// # Errors
    ///
    /// Propagates `std::fs` errors for directory creation or file writing, or TOML serialization errors.
    pub fn write_to_file(&self, path: &Path) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = toml::to_string_pretty(self)?;
        let mut config_file = File::create(path)?;
        config_file.write_all(data.as_bytes())?;
        Ok(())
    }
}

/// Resolves absolute paths for configuration and devices TOML files.
///
/// Defaults to XDG config dir (~/.config/audioleaf/) + default filenames if not provided.
/// Checks file existence (returns bool in tuple) and permissions.
///
/// # Arguments
///
/// * `config_file_path` - Optional override for config.toml path.
/// * `devices_file_path` - Optional override for nl_devices.toml path.
///
/// # Returns
///
/// `Result<((PathBuf, bool), (PathBuf, bool))>` - Resolved paths and their existence flags.
///
/// # Errors
///
/// `anyhow::Error` if insufficient permissions to check path existence.
pub fn resolve_paths(
    config_file_path: Option<PathBuf>,
    devices_file_path: Option<PathBuf>,
) -> Result<((PathBuf, bool), (PathBuf, bool))> {
    let config_file_path = match config_file_path {
        Some(path) => path,
        None => dirs::config_dir()
            .unwrap()
            .join(constants::DEFAULT_CONFIG_DIR)
            .join(constants::DEFAULT_CONFIG_FILE),
    };
    let Ok(config_file_exists) = Path::try_exists(&config_file_path) else {
        bail!(format!(
            "insufficient permissions to access {}",
            config_file_path.to_string_lossy()
        ));
    };
    let devices_file_path = match devices_file_path {
        Some(path) => path,
        None => dirs::config_dir()
            .unwrap()
            .join(constants::DEFAULT_CONFIG_DIR)
            .join(constants::DEFAULT_DEVICES_FILE),
    };
    let Ok(devices_file_exists) = Path::try_exists(&devices_file_path) else {
        bail!(format!(
            "insufficient permissions to access {}",
            devices_file_path.to_string_lossy()
        ));
    };
    Ok((
        (config_file_path, config_file_exists),
        (devices_file_path, devices_file_exists),
    ))
}

/// Interactively discovers Nanoleaf devices via SSDP or accepts manual IP input.
///
/// Performs SSDP M-SEARCH to find devices on network, lists names/IPs for user choice.
/// Supports automatic selection by number, 'M' for manual IP entry, 'Q' to quit.
/// Prompts user to enable pairing mode on device before returning selected IP.
///
/// # Returns
///
/// `Result<Ipv4Addr>` - The chosen device IP address.
///
/// # Errors
///
/// Propagates errors from SSDP discovery, IP parsing, or user abort (bail!).
pub fn get_ip() -> Result<Ipv4Addr> {
    let (names, ips) = ssdp::ssdp_msearch()?;
    let choice = utils::choose_ip(&names)?;
    let ip = match choice {
        utils::Choice::Automatic(i) => ips[i],
        utils::Choice::Manual => match utils::get_ip_from_stdin()? {
            Some(ip) => ip,
            None => bail!("Operation aborted by the user"),
        },
        utils::Choice::Quit => bail!("Operation aborted by the user"),
    };
    println!("Now enable pairing mode on the chosen device (hold its power button until the control lights start flashing).\n
        Press any key when you're ready.");
    utils::wait_for_any_key()?;
    Ok(ip)
}
