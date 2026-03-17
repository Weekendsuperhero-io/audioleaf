use anyhow::{Result, bail};
use palette::{IntoColor, Oklch, Srgb};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    style::{Color, Stylize},
    text::Line,
};
use reqwest::blocking::Client;
use std::{
    fmt::Display,
    io::{self, Stdout, Write, stdout},
    net::Ipv4Addr,
};

pub enum Choice {
    Automatic(usize),
    Manual,
    Quit,
}

/// Prompts the user to select an option from a numbered list interactively.
///
/// Displays the list with indices starting from 1.
/// Accepts input: number (selects index-1), "M" for manual mode, "Q" to quit.
/// Loops until valid input, flushes stdout for immediate prompt visibility.
///
/// Used for selecting discovered Nanoleaf devices by name.
///
/// # Arguments
///
/// * `v` - Slice of displayable items (e.g., device names).
///
/// # Returns
///
/// `Result<Choice>` - Automatic selection with index, Manual, or Quit variant.
pub fn choose_ip(v: &[impl Display]) -> Result<Choice> {
    for (i, option) in v.iter().enumerate() {
        println!("{}. {}", i + 1, option);
    }
    let n = v.len();
    loop {
        print!(
            "Choose an option (by entering its number), enter 'M' to provide the IP adress manually or enter 'Q' to quit: "
        );
        io::stdout().flush()?;
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                if let Ok(x) = input.trim().parse::<usize>()
                    && x >= 1
                    && x <= n
                {
                    return Ok(Choice::Automatic(x - 1));
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

/// Interactively prompts for manual IPv4 address input from stdin.
///
/// Loops reading lines until valid IPv4 or "Q" to quit.
/// Used after SSDP discovery if user chooses manual entry.
///
/// # Returns
///
/// `Result<Option<Ipv4Addr>>` - Parsed IP or None on quit/cancel.
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

/// Pauses execution until any key is pressed, with TUI-friendly handling.
///
/// Temporarily enables raw mode for crossterm event polling.
/// Loops reading events until Key event received (any key).
/// Disables raw mode, prints newline, used for user confirmation prompts (e.g., pairing mode).
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

/// Initializes the terminal user interface (TUI) environment.
///
/// Enables raw mode for input handling, enters alternate screen buffer,
/// creates a new ratatui Terminal with CrosstermBackend on stdout.
///
/// Called before running the app TUI loop.
///
/// # Returns
///
/// `Result<Terminal<CrosstermBackend<Stdout>>>`.
pub fn init_tui() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout())).map_err(anyhow::Error::from)
}

/// Cleans up the TUI environment after app exit.
///
/// Disables raw mode and leaves alternate screen to restore normal terminal state.
/// Called after app run to prevent hanging or corrupted display.
///
/// # Errors
///
/// Propagates crossterm execution errors.
pub fn destroy_tui() -> Result<(), anyhow::Error> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// Generates a user-friendly error message for failed Nanoleaf device connection.
///
/// Formats "couldn't connect to the Nanoleaf device at IP {ip}".
pub fn generate_connection_error_msg(ip: &Ipv4Addr) -> String {
    format!("couldn't connect to the Nanoleaf device at IP {}", ip)
}

/// Performs a blocking POST request to a Nanoleaf API endpoint.
///
/// Optionally includes JSON body data. Executes and checks status, returns response text.
///
/// Used for authenticated API calls requiring POST (e.g., setting effects, panels).
///
/// # Arguments
///
/// * `url` - Full API URL like "http://ip:16021/api/v1/{token}/..." .
/// * `data` - Optional JSON value for request body.
///
/// # Returns
///
/// `Result<String>` - Response body as string, or error if send/status fails.
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

/// Performs a blocking PUT request to a Nanoleaf API endpoint.
///
/// Similar to `request_post`, but uses PUT method. Optional JSON body.
///
/// Used for updating resources like panel colors, effects, or layout.
///
/// # Arguments
///
/// * `url` - API endpoint URL.
/// * `data` - Optional JSON payload.
///
/// # Returns
///
/// `Result<String>` - Response text or error on failure.
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

/// Performs a blocking GET request to a Nanoleaf API endpoint.
///
/// No body, returns response text after status check.
/// Used for retrieving data like device info, effects list, layout, token auth.
///
/// # Arguments
///
/// * `url` - API URL to fetch.
///
/// # Returns
///
/// `Result<String>` - JSON response as string or error.
pub fn request_get(url: &str) -> Result<String> {
    let client = Client::new().get(url);
    let res = client
        .send()?
        .error_for_status()
        .map_err(anyhow::Error::from)?;
    Ok(res.text()?.to_string())
}

/// Generates Oklch base colors from a list of RGB values, expanding or truncating to exact count `n`.
///
/// Repeats last color if fewer than `n`.
/// Each RGB triplet is converted to Oklch preserving the perceptually correct hue, chroma, and lightness.
/// The returned colors represent the **target** appearance at full brightness — the visualizer
/// animates a separate brightness multiplier [0,1] that scales lightness, ensuring the original
/// RGB color is faithfully reproduced at peak audio amplitude.
///
/// Used to map palette colors to panel base colors in visualizer.
pub fn colors_from_rgb(rgb_colors: &[[u8; 3]], n: usize) -> Vec<Oklch> {
    // Spread colors evenly across n panels.  With 4 colors and 12 panels
    // each color covers 3 panels instead of the last color filling 9.
    (0..n)
        .map(|i| {
            let color_idx = i * rgb_colors.len() / n;
            let [r, g, b] = rgb_colors[color_idx];
            let srgb = Srgb::new(r, g, b).into_format::<f32>();
            srgb.into_color()
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

/// Splits a 16-bit unsigned integer into high and low bytes.
///
/// Computes high = x >> 8 (or /256), low = x & 0xFF (or %256).
/// Used for serializing values in Nanoleaf UDP protocol packets.
pub fn split_into_bytes(x: u16) -> (u8, u8) {
    ((x / 256) as u8, (x % 256) as u8)
}

/// Creates a ratatui `Line` with effect name characters styled in cycling colors from a palette.
///
/// Splits string into individual chars, applies foreground color from `colors` array cycling by index.
/// Used when `config.tui_config.colorful_effect_names` is true to highlight effect list items.
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
