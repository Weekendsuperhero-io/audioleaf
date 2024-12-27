use crate::constants;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    crossterm::{
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    Terminal,
};
use reqwest::blocking::Client;
use std::{io::stdout, path::PathBuf};
use url::Url;

pub fn init_tui() -> Result<Terminal<impl Backend>, anyhow::Error> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout())).map_err(anyhow::Error::from)
}

pub fn destroy_tui() -> Result<(), anyhow::Error> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

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

pub fn request_post(url: &str, data: Option<&serde_json::Value>) -> Result<String, anyhow::Error> {
    let url = Url::parse(url)?;
    let mut client = Client::new().post(url);
    if let Some(data) = data {
        client = client.json(&data);
    }
    let res = client
        .send()?
        .error_for_status()
        .map_err(anyhow::Error::from)?;
    Ok(res.text()?.to_string())
}

pub fn request_put(url: &str, data: Option<&serde_json::Value>) -> Result<String, anyhow::Error> {
    let url = Url::parse(url)?;
    let mut client = Client::new().put(url);
    if let Some(data) = data {
        client = client.json(&data);
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
