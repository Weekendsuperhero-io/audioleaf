use crate::constants;
use reqwest::blocking::Client;
use std::path::PathBuf;
use url::Url;

pub fn get_default_cache_dir() -> Result<PathBuf, anyhow::Error> {
    let config_dir = dirs::cache_dir();
    match config_dir {
        Some(dir) => Ok(dir),
        None => {
            let home_dir = dirs::home_dir();
            match home_dir {
                Some(dir) => Ok(dir),
                None => Err(anyhow::Error::msg("Couldn't access user's home directory")),
            }
        }
    }
}

pub fn get_default_config_dir() -> Result<PathBuf, anyhow::Error> {
    let config_dir = dirs::config_dir();
    match config_dir {
        Some(dir) => Ok(dir.join(constants::PROGRAM_NAME)),
        None => {
            let home_dir = dirs::home_dir();
            match home_dir {
                Some(dir) => Ok(dir.join(constants::PROGRAM_NAME)),
                None => Err(anyhow::Error::msg("Couldn't access user's home directory")),
            }
        }
    }
}

pub fn request_post(url: &str, data: Option<&str>) -> Result<String, anyhow::Error> {
    let url = Url::parse(url)?;
    let mut client = Client::new().post(url);
    if let Some(data) = data {
        let json_data: serde_json::Value = serde_json::from_str(data)?;
        client = client.json(&json_data);
    }
    let res = client
        .send()?
        .error_for_status()
        .map_err(anyhow::Error::from)?;
    Ok(res.text()?.to_string())
}

pub fn request_put(url: &str, data: Option<&str>) -> Result<String, anyhow::Error> {
    let url = Url::parse(url)?;
    let mut client = Client::new().put(url);
    if let Some(data) = data {
        let json_data: serde_json::Value = serde_json::from_str(data)?;
        client = client.json(&json_data);
    }
    let res = client
        .send()?
        .error_for_status()
        .map_err(anyhow::Error::from)?;
    Ok(res.text()?.to_string())
}

pub fn request_get(url: &str) -> Result<String, anyhow::Error> {
    let url = Url::parse(url)?;
    let client = Client::new().get(url);
    let res = client
        .send()?
        .error_for_status()
        .map_err(anyhow::Error::from)?;
    Ok(res.text()?.to_string())
}
