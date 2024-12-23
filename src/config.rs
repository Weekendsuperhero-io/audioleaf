use crate::nanoleaf::NanoleafDevice;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::prelude::*;
use std::path::{Path, PathBuf};

pub const PROGRAM_NAME: &str = "audioleaf";
pub const DEFAULT_CONFIG_FILE: &str = "audioleaf.toml";
pub const DEFAULT_NL_DEVICE_FILE: &str = "nl_device";
pub const DEFAULT_HOST_UDP_PORT: u16 = 6789;
pub const DEFAULT_FREQ_RANGE: (u16, u16) = (20, 4500);
pub const DEFAULT_DEFAULT_BOOST: f32 = 1.5;
pub const DEFAULT_HUE_RANGE: (u16, u16) = (210, 390);
pub const DEFAULT_TRANSITION_TIME: u16 = 2;
pub const DEFAULT_AXIS: Axis = Axis::Y;
pub const DEFAULT_SORT: Sort = Sort::Asc;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    Asc,
    Desc,
}

// 'active_panels_ids' are used to order panels - they don't have any connection to each panel's internal multiple-digit ID
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub audio_device: String,
    pub min_freq: u16,
    pub max_freq: u16,
    pub default_boost: f32,
    pub transition_time: u16,
    pub primary_axis: Axis,
    pub sort_primary: Sort,
    pub sort_secondary: Sort,
    pub active_panels_ids: Vec<u16>,
    pub hues: Vec<u16>,
}

fn ok_config_path_or_default(path: Option<&PathBuf>) -> PathBuf {
    match path {
        Some(path) => path.to_owned(),
        None => dirs::config_dir()
            .unwrap()
            .join(PROGRAM_NAME)
            .join(DEFAULT_CONFIG_FILE),
    }
}

pub fn get_from_file(config_file: Option<&PathBuf>) -> Result<Option<Config>, anyhow::Error> {
    let config_file = ok_config_path_or_default(config_file);
    if Path::try_exists(&config_file)? {
        let mut config_file_handle = File::open(config_file)?;
        let mut toml_str = String::new();
        config_file_handle.read_to_string(&mut toml_str)?;

        match toml::from_str(&toml_str) {
            Ok(deserialized_config) => Ok(Some(deserialized_config)),
            Err(e) => Err(anyhow::Error::msg(format!(
                "Parsing the config file failed: {}",
                e
            ))),
        }
    } else {
        Ok(None)
    }
}

pub fn write_and_get_default(
    config_file: Option<&PathBuf>,
    nl_device: &NanoleafDevice,
) -> Result<Config, anyhow::Error> {
    let config_file = ok_config_path_or_default(config_file);
    let config_dir = match config_file.parent() {
        Some(parent) => parent,
        None => {
            return Err(anyhow::Error::msg(format!(
                "Path '{}' is invalid",
                config_file.to_string_lossy()
            )));
        }
    };
    let config = Config {
        audio_device: String::from("default"),
        min_freq: DEFAULT_FREQ_RANGE.0,
        max_freq: DEFAULT_FREQ_RANGE.1,
        default_boost: DEFAULT_DEFAULT_BOOST,
        transition_time: DEFAULT_TRANSITION_TIME,
        primary_axis: DEFAULT_AXIS,
        sort_primary: DEFAULT_SORT,
        sort_secondary: DEFAULT_SORT,
        active_panels_ids: (1..=nl_device.n_panels).collect::<Vec<_>>(),
        hues: (DEFAULT_HUE_RANGE.0..=DEFAULT_HUE_RANGE.1)
            .rev()
            .step_by(
                ((DEFAULT_HUE_RANGE.1 - DEFAULT_HUE_RANGE.0) / (nl_device.n_panels - 1)) as usize,
            )
            .map(|x| x % 360)
            .collect::<Vec<u16>>(),
    };
    let config_toml = toml::to_string_pretty(&config)?;
    if !Path::try_exists(config_dir)? {
        fs::create_dir(config_dir)?;
    }
    let mut config_file_handle = File::create(config_file)?;
    config_file_handle.write_all(config_toml.as_bytes())?;

    Ok(config)
}
