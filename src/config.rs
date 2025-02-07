use crate::constants;
use crate::nanoleaf::{Axis, Sort};
use crate::utils;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::prelude::*;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CliOptions {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub use_colors: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VisualizerOptions {
    pub audio_device: String,
    pub min_freq: u16,
    pub max_freq: u16,
    pub default_gain: f32,
    pub time_window: f32,
    pub transition_time: u16,
    pub primary_axis: Axis,
    pub sort_primary: Sort,
    pub sort_secondary: Sort,
    pub hues: Vec<u16>,
    pub active_panels_numbers: Vec<u16>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub cli_options: CliOptions,
    pub visualizer_options: VisualizerOptions,
}

pub fn resolve_config_file(path: Option<PathBuf>) -> Result<(PathBuf, bool), anyhow::Error> {
    if let Some(path) = path {
        if Path::try_exists(&path)? {
            return Ok((path, true));
        } else {
            return Ok((path, false));
        }
    }
    let default = utils::get_default_config_dir()?.join(constants::DEFAULT_CONFIG_FILE);
    if Path::try_exists(&default)? {
        Ok((default, true))
    } else {
        Ok((default, false))
    }
}

pub fn resolve_nl_device_file(path: Option<PathBuf>) -> Result<(PathBuf, bool), anyhow::Error> {
    if let Some(path) = path {
        if Path::try_exists(&path)? {
            return Ok((path, true));
        } else {
            return Ok((path, false));
        }
    }
    let default = utils::get_default_config_dir()?.join(constants::DEFAULT_NL_DEVICE_FILE);
    if Path::try_exists(&default)? {
        Ok((default, true))
    } else {
        Ok((default, false))
    }
}

pub fn get_config_from_file(config_file: &Path) -> Result<Config, anyhow::Error> {
    let mut config_file_handle = File::open(config_file)?;
    let mut toml_str = String::new();
    config_file_handle.read_to_string(&mut toml_str)?;

    match toml::from_str(&toml_str) {
        Ok(deserialized_config) => Ok(deserialized_config),
        Err(e) => Err(anyhow::Error::msg(format!(
            "Parsing the config file failed: {}",
            e
        ))),
    }
}

pub fn make_default_config(
    config_file: &Path,
    audio_device: String,
    port: u16,
    n_panels: usize,
    ip: Ipv4Addr,
) -> Result<Config, anyhow::Error> {
    let config_dir = match config_file.parent() {
        Some(parent) => parent,
        None => {
            return Err(anyhow::Error::msg(format!(
                "Path '{}' is invalid",
                config_file.to_string_lossy()
            )));
        }
    };
    if !Path::try_exists(config_dir)? {
        fs::create_dir(config_dir)?;
    }

    let cli_options = CliOptions { ip, port, use_colors: true };
    let visualizer_options = VisualizerOptions {
        audio_device,
        min_freq: constants::DEFAULT_FREQ_RANGE.0,
        max_freq: constants::DEFAULT_FREQ_RANGE.1,
        default_gain: constants::DEFAULT_GAIN,
        transition_time: constants::DEFAULT_TRANSITION_TIME,
        time_window: constants::DEFAULT_TIME_WINDOW,
        primary_axis: Axis::default(),
        sort_primary: Sort::default(),
        sort_secondary: Sort::default(),
        active_panels_numbers: (1..=(n_panels as u16)).collect::<Vec<_>>(),
        hues: (constants::DEFAULT_HUE_RANGE.0..=constants::DEFAULT_HUE_RANGE.1)
            .rev()
            .step_by(
                ((constants::DEFAULT_HUE_RANGE.1 - constants::DEFAULT_HUE_RANGE.0)
                    / ((n_panels as u16) - 1)) as usize,
            )
            .map(|x| x % 360)
            .collect::<Vec<u16>>(),
    };
    let config = Config {
        cli_options,
        visualizer_options,
    };
    let config_toml = toml::to_string_pretty(&config)?;
    let mut config_file_handle = File::create(config_file)?;
    config_file_handle.write_all(config_toml.as_bytes())?;

    Ok(config)
}

pub fn get_first_ip(nl_device_file: &Path) -> Result<Ipv4Addr, anyhow::Error> {
    let nl_devices = fs::read_to_string(nl_device_file)?;
    if let Some(device) = nl_devices.lines().next() {
        let split = device.split(';').collect::<Vec<_>>();
        if split.len() != 2 {
            return Err(anyhow::Error::msg(
                "Invalid nl_devices file, every line should look like {IP};{TOKEN}",
            ));
        }
        Ok(split[0].to_string().parse::<Ipv4Addr>()?)
    } else {
        Err(anyhow::Error::msg(
            "Invalid nl_devices file, every line should look like {IP};{TOKEN}",
        ))
    }
}
