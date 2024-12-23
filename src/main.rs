use clap::Parser;
use nanoleaf::NanoleafDevice;
use std::path::PathBuf;

mod config;
mod nanoleaf;

#[derive(Parser, Debug)]
#[command(version, about, author, long_about = None)]
struct CmdOptions {
    /// Audioleaf's configuration file
    #[arg(short, long)]
    config_file: Option<PathBuf>,

    /// File containing the IP, port, and auth token of the Nanoleaf device
    #[arg(short, long)]
    nl_device_file: Option<PathBuf>,

    /// Local IP address of the Nanoleaf device
    #[arg(long)]
    ip: Option<String>,

    /// Port of the UDP socket through which data will be sent to the panels
    #[arg(short, long, requires("ip"))]
    port: Option<u16>,
}

/// TODO:
/// - redirect errors to a file
fn main() -> Result<(), anyhow::Error> {
    let CmdOptions {
        config_file,
        nl_device_file,
        ip,
        port,
    } = CmdOptions::parse();
    if let Some(ip) = ip {
        let port = port.unwrap_or(config::DEFAULT_HOST_UDP_PORT);
        nanoleaf::save_nl_device(ip, port, nl_device_file.as_ref())?;
    }
    let nl_device = NanoleafDevice::new(nl_device_file)?;
    let config = config::get_from_file(config_file.as_ref())?.unwrap_or(
        config::write_and_get_default(config_file.as_ref(), &nl_device)?,
    );
    println!("{:?}", nl_device);
    println!("{:?}", config);

    Ok(())
}
