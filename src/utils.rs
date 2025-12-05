use anyhow::{bail, Result};
use palette::rgb::Srgb;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    crossterm::{
        event::{self, Event},
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

pub enum Choice {
    Automatic(usize),
    Manual,
    Quit,
}

pub fn choose_ip(v: &[impl Display]) -> Result<Choice> {
    for (i, option) in v.iter().enumerate() {
        println!("{}. {}", i + 1, option);
    }
    let n = v.len();
    loop {
        print!("Choose an option (by entering its number), enter 'M' to provide the IP adress manually or enter 'Q' to quit: ");
        io::stdout().flush()?;
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                if let Ok(x) = input.trim().parse::<usize>() {
                    if x >= 1 && x <= n {
                        return Ok(Choice::Automatic(x - 1));
                    }
                }
                if input.trim() == "M" {
                    return Ok(Choice::Manual);
                }
                if input.trim() == "Q" {
                    return Ok(Choice::Quit);
                }
            }
            Err(e) => bail!(e),
        }
    }
}

pub fn get_ip_from_stdin() -> Result<Option<Ipv4Addr>> {
    loop {
        print!("Enter the local IP address of your Nanoleaf device or 'Q' to quit: ");
        io::stdout().flush()?;
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                if let Ok(ip) = input.trim().parse::<Ipv4Addr>() {
                    return Ok(Some(ip));
                }
                if input.trim() == "Q" {
                    return Ok(None);
                }
            }
            Err(e) => bail!(e),
        }
    }
}

pub fn wait_for_any_key() -> Result<()> {
    // Enable raw mode temporarily to detect key presses
    enable_raw_mode()?;

    // Wait for any key press using crossterm event handling
    loop {
        if let Event::Key(_) = event::read()? {
            break;
        }
    }

    // Disable raw mode and return to normal terminal
    disable_raw_mode()?;
    println!(); // Add newline after key press
    Ok(())
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
    format!("couldn't connect to the Nanoleaf device at IP {}", ip)
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
        .map(|hue| {
            // Use special hue value 360 to represent white (high whiteness)
            if hue == 360 {
                palette::Hwb::new(0.1, 1.0, 0.0) // White: any hue, high whiteness, starts black
            } else {
                palette::Hwb::new(hue as f32, 0.0, 1.0) // Normal colors: no whiteness
            }
        })
        .collect()
}

/// adjust the amplitude to account for the sound perception
/// using the Equal Loudness Contour (ISO 226)
/// values from github.com/musios-app/equal-loudness
pub fn equalize(a: f32, f: u32) -> f32 {
    match f {
        x if x <= 23 => 0.40 * a,
        x if x <= 28 => 0.43 * a,
        x if x <= 36 => 0.45 * a,
        x if x <= 45 => 0.48 * a,
        x if x <= 57 => 0.52 * a,
        x if x <= 72 => 0.55 * a,
        x if x <= 91 => 0.58 * a,
        x if x <= 113 => 0.62 * a,
        x if x <= 142 => 0.66 * a,
        x if x <= 180 => 0.70 * a,
        x if x <= 225 => 0.75 * a,
        x if x <= 283 => 0.79 * a,
        x if x <= 352 => 0.84 * a,
        x if x <= 450 => 0.89 * a,
        x if x <= 565 => 0.93 * a,
        x if x <= 715 => 0.98 * a,
        x if x <= 1125 => 1.00 * a,
        x if x <= 1425 => 0.95 * a,
        x if x <= 1800 => 0.94 * a,
        x if x <= 2250 => 1.02 * a,
        x if x <= 2825 => 1.10 * a,
        x if x <= 3575 => 1.12 * a,
        x if x <= 4500 => 1.09 * a,
        x if x <= 5650 => 1.00 * a,
        x if x <= 7150 => 0.86 * a,
        x if x <= 9000 => 0.77 * a,
        x if x <= 11_250 => 0.74 * a,
        x if x <= 14_250 => 0.78 * a,
        x if x <= 18_000 => 0.77 * a,
        _ => 1.0 / 0.43 * a,
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
