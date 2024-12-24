use crate::nanoleaf::NanoleafDevice;
use crate::constants;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    #[default]
    Y,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
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

pub fn ok_path_or_default(path: Option<&PathBuf>) -> PathBuf {
    match path {
        Some(path) => path.to_owned(),
        None => dirs::config_dir()
            .unwrap()
            .join(constants::PROGRAM_NAME)
            .join(constants::DEFAULT_CONFIG_FILE),
    }
}

pub fn get_from_file(config_file: Option<&PathBuf>) -> Result<Option<Config>, anyhow::Error> {
    let config_file = ok_path_or_default(config_file);
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
    let config_file = ok_path_or_default(config_file);
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
        min_freq: constants::DEFAULT_FREQ_RANGE.0,
        max_freq: constants::DEFAULT_FREQ_RANGE.1,
        default_boost: constants::DEFAULT_DEFAULT_BOOST,
        transition_time: constants::DEFAULT_TRANSITION_TIME,
        primary_axis: Axis::default(),
        sort_primary: Sort::default(),
        sort_secondary: Sort::default(),
        active_panels_ids: (1..=nl_device.n_panels).collect::<Vec<_>>(),
        hues: (constants::DEFAULT_HUE_RANGE.0..=constants::DEFAULT_HUE_RANGE.1)
            .rev()
            .step_by(
                ((constants::DEFAULT_HUE_RANGE.1 - constants::DEFAULT_HUE_RANGE.0) / (nl_device.n_panels - 1)) as usize,
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
