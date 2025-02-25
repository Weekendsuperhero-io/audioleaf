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
}

#[derive(Debug, Serialize)]
pub struct TuiConfig {
    pub colorful_effect_names: Option<bool>,
}

impl TuiConfig {
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

#[derive(Debug, Default, Serialize)]
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
    pub transition_time: Option<u16>,
    pub time_window: Option<f32>,
    pub primary_axis: Option<Axis>,
    pub sort_primary: Option<Sort>,
    pub sort_secondary: Option<Sort>,
}

impl VisualizerConfig {
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
                ("hues", Value::Array(v)) => {
                    if v.is_empty() {
                        bail!("hues cannot be an empty array");
                    }
                    if v.iter().map(|x| x.as_integer()).any(|x| match x.as_ref() {
                        Some(x) => !(0..360).contains(x),
                        None => true,
                    }) {
                        bail!("hues must be integers from 0 to 359 inclusive");
                    }
                    let hues: Vec<u16> = v
                        .into_iter()
                        .map(|x| u16::try_from(x.as_integer().unwrap()).unwrap())
                        .collect();
                    visualizer_config.hues = Some(hues);
                }
                ("default_gain", Value::Float(x)) => {
                    visualizer_config.default_gain = Some(x as f32);
                }
                ("transition_time", Value::Integer(x)) => {
                    visualizer_config.transition_time = Some(u16::try_from(x)?);
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

    pub fn parse_from_file(path: &Path) -> Result<Self> {
        let mut config_file = File::open(path)?;
        let mut contents = String::new();
        config_file.read_to_string(&mut contents)?;
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

    pub fn write_to_file(&self, path: &Path) -> Result<()> {
        let data = toml::to_string_pretty(self)?;
        let mut config_file = File::create(path)?;
        config_file.write_all(data.as_bytes())?;
        Ok(())
    }
}

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
