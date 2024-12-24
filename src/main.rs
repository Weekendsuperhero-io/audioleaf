use clap::Parser;
use nanoleaf::NanoleafDevice;
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
    #[arg(long)]
    ssdp: Option<u64>,

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
        ssdp,
        config_file,
        nl_device_file,
        ip,
        port,
    } = CmdOptions::parse();
    if let Some(ssdp_timeout) = ssdp {
        ssdp::ssdp_msearch(ssdp_timeout)?;
        return Ok(());
    }
    if let Some(ip) = ip {
        // TODO: to allow connecting to different devices, keep every device's info in
        // a file called "nl_device_{6 char hash of the IP}
        nanoleaf::save_nl_device(ip, nl_device_file.as_ref())?;
    }
    let port = port.unwrap_or(constants::DEFAULT_HOST_UDP_PORT);
    let nl_device = NanoleafDevice::new(nl_device_file.as_ref())?; // how to know which ip the user is trying to connect to?
    let config = config::get_from_file(config_file.as_ref())?.unwrap_or(
        config::write_and_get_default(config_file.as_ref(), &nl_device)?,
    );
    println!("{:?}", nl_device);
    println!("{:?}", config);

    Ok(())
}
