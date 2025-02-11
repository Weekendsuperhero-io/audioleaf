use anyhow::Result;
use clap::Parser;

mod app;
mod audio;
mod config;
mod constants;
mod event_handler;
mod nanoleaf;
mod panic;
mod processing;
mod ssdp;
mod utils;
mod visualizer;

fn main() -> Result<()> {
    panic::register_backtrace_panic_handler();
    let config::CliOptions {
        config_file_path,
        devices_file_path,
        device_name,
        add_new,
    } = config::CliOptions::parse();
    let ((config_file_path, config_file_exists), (devices_file_path, devices_file_exists)) =
        config::resolve_paths(config_file_path, devices_file_path)?;
    let (nl_device, tui_config, visualizer_config) = if !add_new && devices_file_exists {
        if config_file_exists {
            let config = config::Config::parse_from_file(&config_file_path)?;
            let name_to_search = if device_name.is_some() {
                &device_name
            } else {
                &config.default_nl_device_name
            };
            let nl_device =
                nanoleaf::NlDevice::find_in_file(&devices_file_path, name_to_search.as_deref())?;
            (nl_device, config.tui_config, config.visualizer_config)
        } else {
            let nl_device =
                nanoleaf::NlDevice::find_in_file(&devices_file_path, device_name.as_deref())?;
            let config = config::Config::new(Some(nl_device.name.clone()), None, None);
            config.write_to_file(&config_file_path)?;
            (nl_device, config.tui_config, config.visualizer_config)
        }
    } else {
        let ip = config::get_ip_from_stdin()?;
        let nl_device = nanoleaf::NlDevice::new(ip)?;
        nl_device.append_to_file(&devices_file_path)?;
        let config = if config_file_exists {
            let mut config = config::Config::parse_from_file(&config_file_path)?;
            config.default_nl_device_name = Some(nl_device.name.clone());
            config
        } else {
            config::Config::new(Some(nl_device.name.clone()), None, None)
        };
        config.write_to_file(&config_file_path)?;
        (nl_device, config.tui_config, config.visualizer_config)
    };
    let mut app = app::App::new(nl_device, tui_config, visualizer_config)?;
    let mut terminal = utils::init_tui()?;
    app.run(&mut terminal)?;
    utils::destroy_tui()?;
    Ok(())
}
