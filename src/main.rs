use crate::nanoleaf::NanoleafDevice;
use clap::Parser;
use std::path::PathBuf;

mod config;
mod constants;
mod nanoleaf;
mod ssdp;

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
    #[arg(short, long, requires("ip"))]
    port: Option<u16>,
}

/// TODO:
/// - redirect errors to a file
fn main() -> Result<(), anyhow::Error> {
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
        if let Some(ip) = ip {
            config.ip = ip;
        }
        if let Some(port) = port {
            config.port = port;
        }
        let nl = NanoleafDevice::new(&config.ip, &nl_device_file)?;
        (nl, config)
    } else {
        let ip = if let Some(ip) = ip {
            ip
        } else {
            if !nl_device_file_exists {
                return Err(anyhow::Error::msg("No configuration files found - find out the IP address of your Nanoleaf device\n\
                        (which you can do by running `audioleaf --ssdp` to discover all available devices)\n\
                        and then run `audioleaf --ip <IP>` while the control lights on your main panel are blinking.\n\
                        For more details refer to the README."));
            }
            config::get_first_ip(&nl_device_file)?
        };
        let nl = NanoleafDevice::new(&ip, &nl_device_file)?;
        let config = config::make_default_config(&config_file, &nl, port)?;
        (nl, config)
    };
    nl.sort_panels(
        config.primary_axis,
        config.sort_primary,
        config.sort_secondary,
    );
    println!("{:?}", nl);
    println!("{:?}", config);
    // let app = App:new(nl, config);
    // app.run()?;

    Ok(())
}
