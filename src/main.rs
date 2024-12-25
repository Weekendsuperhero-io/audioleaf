use crate::app::App;
use crate::nanoleaf::NanoleafDevice;
use clap::Parser;
use std::path::PathBuf;

mod app;
mod config;
mod constants;
mod nanoleaf;
mod panic;
mod ssdp;
mod utils;

#[derive(Parser, Debug)]
#[command(version, about, author, long_about = None)]
struct CmdOptions {
    /// If passed, audioleaf will only try to discover Nanoleaf devices
    /// present on the local network (SSDP must be enabled).
    /// The value specifies the time (in seconds) before audioleaf will give up the search.
    #[arg(long, require_equals = true, num_args = 0..=1, default_value = "0", default_missing_value = "10")]
    ssdp: u64,

    /// Local IP address of the Nanoleaf device
    #[arg(long)]
    ip: Option<String>,

    /// Audioleaf's configuration file
    #[arg(short, long)]
    config_file: Option<PathBuf>,

    /// File containing the IP, port, and auth token of the Nanoleaf device
    #[arg(short, long)]
    nl_device_file: Option<PathBuf>,

    /// Port of the UDP socket through which data will be sent to the panels
    #[arg(short, long)]
    port: Option<u16>,
}

fn main() -> Result<(), anyhow::Error> {
    panic::register_backtrace_panic_handler();
    let CmdOptions {
        ssdp,
        ip,
        config_file,
        nl_device_file,
        port,
    } = CmdOptions::parse();
    if ssdp > 0 {
        ssdp::ssdp_msearch(ssdp)?;
        return Ok(());
    }
    let (nl_device_file, nl_device_file_exists) = config::resolve_nl_device_file(nl_device_file)?;
    let (config_file, config_file_exists) = config::resolve_config_file(config_file)?;
    let (mut nl, config) = if config_file_exists {
        let mut config = config::get_config_from_file(&config_file)?;
        println!("Config file {} found!", config_file.to_string_lossy());
        if let Some(ip) = ip {
            config.ip = ip;
        }
        if let Some(port) = port {
            config.port = port;
        }
        let nl = NanoleafDevice::new(&config.ip, &nl_device_file)?;
        (nl, config)
    } else {
        println!("No config file found!");
        let ip = if let Some(ip) = ip {
            ip
        } else {
            if !nl_device_file_exists {
                return Err(anyhow::Error::msg(format!("You don't have any Nanoleaf devices saved in your connection history ({} file) - find out\n\
                        the IP address of your Nanoleaf device\n (which you can do by running `audioleaf --ssdp` to discover all available devices)\n\
                        and then run `audioleaf --ip <IP>` while the control lights on your main panel are flashing.\n\
                        For more details refer to the README.", nl_device_file.to_string_lossy())));
            }
            config::get_first_ip(&nl_device_file)?
        };
        let nl = NanoleafDevice::new(&ip, &nl_device_file)?;
        let config = config::make_default_config(&config_file, &nl, port)?;
        println!(
            "Default configuration saved to {}!",
            config_file.to_string_lossy()
        );
        (nl, config)
    };
    println!("Connected to {}!", nl.name);
    nl.sort_panels(
        config.primary_axis,
        config.sort_primary,
        config.sort_secondary,
    );

    let mut terminal = ratatui::init();
    let mut app = App::new(nl, config)?;
    app.run(&mut terminal)?;
    ratatui::restore();
    Ok(())
}
