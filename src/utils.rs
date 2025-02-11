use anyhow::{anyhow, Result};
use palette::rgb::Srgb;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    crossterm::{
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    style::{Color, Stylize},
    text::Line,
    Terminal,
};
use reqwest::blocking::Client;
use std::{
    fmt::Display,
    io::{self, stdout, Write},
    net::Ipv4Addr,
};

pub fn ask_choose_one(v: &[impl Display]) -> Result<Option<usize>> {
    for (i, option) in v.iter().enumerate() {
        println!("{}. {}", i + 1, option);
    }
    let n = v.len();
    loop {
        print!("Choose an option (by entering its number) or enter 'Q' to quit: ");
        io::stdout().flush()?;
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                if let Ok(x) = input.trim().parse::<usize>() {
                    if x >= 1 && x <= n {
                        return Ok(Some(x - 1));
                    }
                }
                if input.trim() == "Q" {
                    return Ok(None);
                }
            }
            Err(e) => return Err(anyhow!(e)),
        }
    }
}

pub fn wait_for_any_key() -> Result<()> {
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow!(e)),
    }
}

pub fn init_tui() -> Result<Terminal<impl Backend>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout())).map_err(anyhow::Error::from)
}

pub fn destroy_tui() -> Result<(), anyhow::Error> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

pub fn generate_connection_error_msg(ip: &Ipv4Addr) -> String {
    format!(
        "Error: couldn't connect to the Nanoleaf device at IP {}",
        ip
    )
}

pub fn request_post(url: &str, data: Option<&serde_json::Value>) -> Result<String> {
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

pub fn request_put(url: &str, data: Option<&serde_json::Value>) -> Result<String> {
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

pub fn request_get(url: &str) -> Result<String> {
    let client = Client::new().get(url);
    let res = client
        .send()?
        .error_for_status()
        .map_err(anyhow::Error::from)?;
    Ok(res.text()?.to_string())
}

pub fn colors_from_hues(hues: &[u16], n: usize) -> Vec<palette::Hwb> {
    let mut res = Vec::from(hues);
    res.resize(n, *hues.last().unwrap());
    res.into_iter()
        .map(|hue| palette::Hwb::new(hue as f32, 0.0, 1.0))
        .collect()
}

/// adjust the amplitude to account for the sound perception
/// using the Equal Loudness Contour (ISO 226)
/// values from github.com/musios-app/equal-loudness
pub fn equalize(a: f32, f: u32) -> f32 {
    match f {
        x if x <= 23 => 1.0 / 2.50 * a,
        x if x <= 28 => 1.0 / 2.35 * a,
        x if x <= 36 => 1.0 / 2.20 * a,
        x if x <= 45 => 1.0 / 2.07 * a,
        x if x <= 57 => 1.0 / 1.94 * a,
        x if x <= 72 => 1.0 / 1.83 * a,
        x if x <= 91 => 1.0 / 1.71 * a,
        x if x <= 113 => 1.0 / 1.61 * a,
        x if x <= 142 => 1.0 / 1.51 * a,
        x if x <= 180 => 1.0 / 1.42 * a,
        x if x <= 225 => 1.0 / 1.34 * a,
        x if x <= 283 => 1.0 / 1.26 * a,
        x if x <= 352 => 1.0 / 1.19 * a,
        x if x <= 450 => 1.0 / 1.12 * a,
        x if x <= 565 => 1.0 / 1.08 * a,
        x if x <= 715 => 1.0 / 1.03 * a,
        x if x <= 1125 => a,
        x if x <= 1425 => 1.0 / 1.05 * a,
        x if x <= 1800 => 1.0 / 1.06 * a,
        x if x <= 2250 => 1.0 / 0.98 * a,
        x if x <= 2825 => 1.0 / 0.91 * a,
        x if x <= 3575 => 1.0 / 0.89 * a,
        x if x <= 4500 => 1.0 / 0.92 * a,
        x if x <= 5650 => a,
        x if x <= 7150 => 1.0 / 1.15 * a,
        x if x <= 9000 => 1.0 / 1.30 * a,
        x if x <= 11_250 => 1.0 / 1.36 * a,
        x if x <= 14_250 => 1.0 / 1.29 * a,
        x if x <= 18_000 => 1.0 / 1.30 * a,
        _ => 1.0 / 2.32 * a,
    }
}

pub fn split_into_bytes(x: u16) -> (u8, u8) {
    ((x / 256) as u8, (x % 256) as u8)
}

pub fn colorful_effect_name<'a>(effect_name: &'a str, colors: &'a [Srgb<u8>]) -> Line<'a> {
    let chars = effect_name.chars().map(|c| c.to_string());
    Line::from(
        chars
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let color = colors[i % colors.len()];
                c.fg(Color::Rgb(color.red, color.green, color.blue))
            })
            .collect::<Vec<_>>(),
    )
}
