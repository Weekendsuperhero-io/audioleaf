use crate::app::App;
use crate::nanoleaf::NanoleafDevice;
use clap::Parser;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use cli_log::*;

mod app;
mod audio;
mod config;
mod constants;
mod nanoleaf;
mod panic;
mod ssdp;
mod utils;
mod visualizer;

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
    ip: Option<Ipv4Addr>,

    /// Audioleaf's configuration file
    #[arg(short, long)]
    config_file: Option<PathBuf>,

    /// File containing the IP, port, and auth token of the Nanoleaf device
    #[arg(short, long)]
    nl_device_file: Option<PathBuf>,

    /// Port of the UDP socket through which data will be sent to the panels
    #[arg(short, long)]
    port: Option<u16>,

    /// Audio device to use as the source of data for the visualizer.
    /// Use "none" if you don't wish to use this feature.
    #[arg(short, long)]
    audio_device: Option<String>,
}

fn main() -> Result<(), anyhow::Error> {
    init_cli_log!();
    let CmdOptions {
        ssdp,
        ip,
        config_file,
        nl_device_file,
        port,
        audio_device,
    } = CmdOptions::parse();
    if ssdp > 0 {
        ssdp::ssdp_msearch(ssdp)?;
        return Ok(());
    }
    let (nl_device_file, nl_device_file_exists) = config::resolve_nl_device_file(nl_device_file)?;
    let (config_file, config_file_exists) = config::resolve_config_file(config_file)?;
    let (mut nl, config) = if config_file_exists {
        let mut config = config::get_config_from_file(&config_file)?;
        println!("Config file {} found", config_file.to_string_lossy());
        if let Some(ip) = ip {
            config.ip = ip;
        }
        if let Some(port) = port {
            config.port = port;
        }
        if let Some(audio_device) = audio_device {
            config.audio_device = audio_device;
        }
        let nl = NanoleafDevice::new(&config.ip, &nl_device_file)?;
        (nl, config)
    } else {
        println!("No config file found");
        let ip = if let Some(ip) = ip {
            ip
        } else {
            if !nl_device_file_exists {
                return Err(anyhow::Error::msg(format!("You don't have any Nanoleaf devices saved in your connection history ({} file).\n\
                            Here's what to do:\n\
                        -\tFind the IP address of your Nanoleaf device (which you can do by, for example, running `audioleaf --ssdp`)\n\
                        -\tRun `audioleaf --ip <IP>` while your device is in \"pairing mode\". (You can enable this mode by\n\
                        \tpressing and holding down the power button for ~5s - once the control lights start flashing\n\
                        \tcyclically that means paring mode is on. \"Pairing mode\" will turn off automatically after some time.)\n\
                        For more details refer to the README.", nl_device_file.to_string_lossy())));
            }
            config::get_first_ip(&nl_device_file)?
        };
        let audio_device = audio_device.unwrap_or(constants::DEFAULT_AUDIO_DEVICE.to_string());
        let port = port.unwrap_or(constants::DEFAULT_HOST_UDP_PORT);
        let nl = NanoleafDevice::new(&ip, &nl_device_file)?;
        let config = config::make_default_config(&config_file, &nl, audio_device, port)?;
        println!(
            "Default configuration saved to {}",
            config_file.to_string_lossy()
        );
        (nl, config)
    };
    println!("Connected to {}", nl.name);
    nl.sort_panels(
        config.primary_axis,
        config.sort_primary,
        config.sort_secondary,
    );

    let (device, sample_format, stream_config) =
        visualizer::setup_audio_device(&config.audio_device)?;
    println!("Using audio device \"{}\"", config.audio_device);
    // config::validate(&config, ...)?; // for example check if hues are in 0..=360, max_freq is in range, ...
    let panels = nl.panels.clone();
    let udp_socket = nl.get_udp_socket(config.port)?;
    let (visualizer_thread, tx) = visualizer::setup_visualizer_thread(
        device,
        sample_format,
        stream_config,
        &config,
        panels,
        udp_socket
    )?;

    // install a custom panic hook so that the terminal doesn't get messed up
    // and the user can access the backtrace
    panic::register_backtrace_panic_handler();
    let mut terminal = utils::init_tui()?;
    let mut app = App::new(nl, tx)?;
    app.run(&mut terminal)?;
    visualizer_thread.join().unwrap();
    utils::destroy_tui()?;
    Ok(())
}
