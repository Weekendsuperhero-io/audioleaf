use crate::{constants, ssdp, utils};
use anyhow::{bail, Result};
use clap::Parser;
use serde::Serialize;
use std::fs::File;
use std::{
    io::prelude::*,
    net::Ipv4Addr,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

#[derive(Parser, Debug)]
#[command(version, about, author, long_about = None)]
pub struct CliOptions {
    /// Path to audioleaf's configuration file
    #[arg(long = "config")]
    pub config_file_path: Option<PathBuf>,

    /// Path to audioleaf's database of known Nanoleaf devices
    #[arg(long = "devices")]
    pub devices_file_path: Option<PathBuf>,

    /// Name of the Nanoleaf device to connect to (e.g. Canvas 2E50)
    #[arg(short = 'd', long = "device-name")]
    pub device_name: Option<String>,

    /// Explicitly add a new Nanoleaf device
    #[arg(short = 'n', long = "new")]
    pub add_new: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Parser, Debug)]
pub enum Command {
    /// Dump information from device or configuration
    Dump {
        #[command(subcommand)]
        dump_type: DumpType,
    },
}

#[derive(Parser, Debug)]
pub enum DumpType {
    /// Dump panel layout information from the device
    Layout,
    /// Dump available color palettes
    Palettes,
    /// Dump device info from /api/v1/ endpoint (no auth required)
    Info,
    /// Show graphical panel layout visualization
    LayoutGraphical,
}

#[derive(Debug, Serialize)]
pub struct TuiConfig {
    pub colorful_effect_names: Option<bool>,
}

impl TuiConfig {
    /// Returns the default TUI configuration.
    ///
    /// Sets `colorful_effect_names` to the constant `DEFAULT_COLORFUL_EFFECT_NAMES` (typically false,
    /// meaning effect names in the effect list are displayed without per-character coloring based on palette).
    pub fn default() -> Self {
        TuiConfig {
            colorful_effect_names: Some(constants::DEFAULT_COLORFUL_EFFECT_NAMES),
        }
    }
}

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

#[derive(Debug, Serialize)]
pub struct VisualizerConfig {
    pub audio_backend: Option<String>,
    pub freq_range: Option<(u16, u16)>,
    pub hues: Option<Vec<u16>>,
    pub default_gain: Option<f32>,
    pub transition_time: Option<i16>,
    pub time_window: Option<f32>,
    pub primary_axis: Option<Axis>,
    pub sort_primary: Option<Sort>,
    pub sort_secondary: Option<Sort>,
}

impl VisualizerConfig {
    /// Returns the default visualizer configuration.
    ///
    /// Initializes with constants:
    /// - `audio_backend`: "default"
    /// - `freq_range`: (20, 4500) Hz
    /// - `hues`: Standard rainbow-like [30,0,330,...]
    /// - `default_gain`: 1.0
    /// - `transition_time`: 2 (200ms)
    /// - `time_window`: 0.1875 s
    /// - Sorting: Y axis ascending, secondary ascending
    pub fn default() -> Self {
        VisualizerConfig {
            audio_backend: Some("default".to_string()),
            freq_range: Some(constants::DEFAULT_FREQ_RANGE),
            hues: Some(vec![30, 0, 330, 300, 270, 240, 210]),
            default_gain: Some(constants::DEFAULT_GAIN),
            transition_time: Some(constants::DEFAULT_TRANSITION_TIME),
            time_window: Some(constants::DEFAULT_TIME_WINDOW),
            primary_axis: Some(Axis::default()),
            sort_primary: Some(Sort::default()),
            sort_secondary: Some(Sort::default()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Config {
    pub default_nl_device_name: Option<String>,
    pub tui_config: TuiConfig,
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
    /// * `tui_config` - Optional TUI settings; defaults to `TuiConfig::default()`.
    /// * `visualizer_config` - Optional visualizer params; defaults to `VisualizerConfig::default()`.
    pub fn new(
        default_nl_device_name: Option<String>,
        tui_config: Option<TuiConfig>,
        visualizer_config: Option<VisualizerConfig>,
    ) -> Self {
        Config {
            default_nl_device_name,
            tui_config: tui_config.unwrap_or(TuiConfig::default()),
            visualizer_config: visualizer_config.unwrap_or(VisualizerConfig::default()),
        }
    }

    /// Parses a TOML table into the fields of a mutable `TuiConfig`.
    ///
    /// Supports key "colorful_effect_names" as boolean value.
    /// Ignores unknown keys? No, bails with error on invalid keys.
    /// Updates `tui_config` in place.
    ///
    /// # Arguments
    ///
    /// * `tui_config` - Mutable reference to populate.
    /// * `t` - TOML table from config section.
    ///
    /// # Errors
    ///
    /// `anyhow::Error` for invalid key types or unknown keys.
    pub fn parse_tui_config(tui_config: &mut TuiConfig, t: toml::Table) -> Result<()> {
        for (key, val) in t {
            match (key.as_str(), val) {
                ("colorful_effect_names", Value::Boolean(b)) => {
                    tui_config.colorful_effect_names = Some(b);
                }
                (key, _) => {
                    bail!(format!("invalid key `{}`", key));
                }
            }
        }
        Ok(())
    }

    /// Parses a TOML table into the fields of a mutable `VisualizerConfig`.
    ///
    /// Supports comprehensive field validation and type conversion:
    /// - `audio_backend`: String for device name.
    /// - `freq_range`: 2-element array of u16 [min_hz, max_hz].
    /// - `hues`: Array of u16 (0-360) or string name of predefined palette (e.g., "ocean-nightclub").
    /// - `default_gain`: f32 or i64, applied to spectrum amplitudes.
    /// - `transition_time`: i16 (-1 for instant, positive in 100ms units for Nanoleaf transitions).
    /// - `time_window`: f32 seconds for smoothing window.
    /// - `primary_axis`: "X" or "Y" enum.
    /// - `sort_primary`/`sort_secondary`: "Asc" or "Desc".
    ///
    /// Validates ranges (e.g., hues 0-360, transition_time >= -1) and bails on errors or unknown keys.
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
                ("hues", Value::String(s)) => {
                    // Named palette support
                    match crate::palettes::get_palette(&s) {
                        Some(hues) => visualizer_config.hues = Some(hues),
                        None => {
                            let available = crate::palettes::get_palette_names().join(", ");
                            bail!(
                                "Unknown palette name '{}'. Available palettes: {}",
                                s,
                                available
                            );
                        }
                    }
                }
                ("hues", Value::Array(v)) => {
                    if v.is_empty() {
                        bail!("hues cannot be an empty array");
                    }
                    if v.iter().map(|x| x.as_integer()).any(|x| match x.as_ref() {
                        Some(x) => !(0..=360).contains(x),
                        None => true,
                    }) {
                        bail!("hues must be integers from 0 to 360 inclusive (360 = white)");
                    }
                    let hues: Vec<u16> = v
                        .into_iter()
                        .map(|x| u16::try_from(x.as_integer().unwrap()).unwrap())
                        .collect();
                    visualizer_config.hues = Some(hues);
                }
                ("default_gain", Value::Float(x)) => {
                    eprintln!("DEBUG: Parsed default_gain as Float: {}", x);
                    visualizer_config.default_gain = Some(x as f32);
                }
                ("default_gain", Value::Integer(x)) => {
                    eprintln!("DEBUG: Parsed default_gain as Integer: {}", x);
                    visualizer_config.default_gain = Some(x as f32);
                }
                ("transition_time", Value::Integer(x)) => {
                    let trans_time = i16::try_from(x)?;
                    if trans_time < -1 {
                        bail!("transition_time must be -1 (instant) or a positive value. Note: units are in 100ms (1 = 100ms, 2 = 200ms, etc.)");
                    }
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
    pub fn parse_from_file(path: &Path) -> Result<Self> {
        eprintln!("DEBUG: Reading config from: {}", path.display());
        let mut config_file = File::open(path)?;
        let mut contents = String::new();
        config_file.read_to_string(&mut contents)?;
        eprintln!("DEBUG: Config file contents:\n{}", contents);
        let data = contents.parse::<Table>()?;

        let mut default_nl_device_name = None;
        let mut tui_config = TuiConfig::default();
        let mut visualizer_config = VisualizerConfig::default();
        for (key, val) in data {
            match (key.as_str(), val) {
                ("default_nl_device_name", Value::String(s)) => {
                    default_nl_device_name = Some(s);
                }
                ("tui_config", Value::Table(t)) => {
                    Self::parse_tui_config(&mut tui_config, t)?;
                }
                ("visualizer_config", Value::Table(t)) => {
                    Self::parse_visualizer_config(&mut visualizer_config, t)?;
                }
                (key, _) => {
                    bail!(format!("invalid key `{}`", key));
                }
            }
        }
        Ok(Config::new(
            default_nl_device_name,
            Some(tui_config),
            Some(visualizer_config),
        ))
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
