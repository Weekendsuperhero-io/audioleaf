use anyhow::Result;
use clap::Parser;

mod app;
mod audio;
mod config;
mod constants;
mod event_handler;
mod graphical_layout;
mod layout_visualizer;
mod nanoleaf;
mod palettes;
mod panic;
mod processing;
mod ssdp;
mod utils;
mod visualizer;

fn main() -> Result<()> {
    pollster::block_on(main_async())
}

async fn main_async() -> Result<()> {
    panic::register_backtrace_panic_handler();
    let cli_options = config::CliOptions::parse();

    // Handle dump commands separately - they don't need TUI
    if let Some(config::Command::Dump { dump_type }) = &cli_options.command {
        return handle_dump_command(dump_type, &cli_options).await;
    }

    let config::CliOptions {
        config_file_path,
        devices_file_path,
        device_name,
        add_new,
        ..
    } = cli_options;
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
        let ip = config::get_ip()?;
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

    // Ensure device is powered on and has brightness set
    nl_device.ensure_device_ready()?;

    let mut app = app::App::new(nl_device, tui_config, visualizer_config)?;
    let mut terminal = utils::init_tui()?;
    app.run(&mut terminal)?;
    utils::destroy_tui()?;
    Ok(())
}

async fn handle_dump_command(
    dump_type: &config::DumpType,
    cli_options: &config::CliOptions,
) -> Result<()> {
    match dump_type {
        config::DumpType::Layout => {
            // Need to connect to device for layout
            let ((config_file_path, config_file_exists), (devices_file_path, devices_file_exists)) =
                config::resolve_paths(
                    cli_options.config_file_path.clone(),
                    cli_options.devices_file_path.clone(),
                )?;

            if !devices_file_exists {
                anyhow::bail!("No devices file found. Please add a device first.");
            }

            let nl_device = if config_file_exists {
                let config = config::Config::parse_from_file(&config_file_path)?;
                let name_to_search = if cli_options.device_name.is_some() {
                    &cli_options.device_name
                } else {
                    &config.default_nl_device_name
                };
                nanoleaf::NlDevice::find_in_file(&devices_file_path, name_to_search.as_deref())?
            } else {
                nanoleaf::NlDevice::find_in_file(
                    &devices_file_path,
                    cli_options.device_name.as_deref(),
                )?
            };

            println!("Panel Layout Information for: {}", nl_device.name);
            println!("Device IP: {}", nl_device.ip);

            let layout = nl_device.get_panel_layout()?;
            let orientation = nl_device.get_global_orientation()?;
            let global_orientation = orientation["value"].as_u64().unwrap_or(0) as u16;

            // Parse and visualize the layout
            let panels = layout_visualizer::parse_layout(&layout)?;
            layout_visualizer::visualize_layout(&panels, global_orientation);

            println!("\n=== Raw Panel Layout JSON ===");
            println!("{}", serde_json::to_string_pretty(&layout)?);

            println!("\n=== Raw Global Orientation JSON ===");
            println!("{}", serde_json::to_string_pretty(&orientation)?);

            Ok(())
        }
        config::DumpType::Palettes => {
            println!("Available Color Palettes:\n");
            let palette_names = palettes::get_palette_names();
            for name in palette_names {
                let hues = palettes::get_palette(&name).unwrap();
                println!("  {} = {:?}", name, hues);
            }
            Ok(())
        }
        config::DumpType::LayoutGraphical => {
            // Need to connect to device for layout
            let ((config_file_path, config_file_exists), (devices_file_path, devices_file_exists)) =
                config::resolve_paths(
                    cli_options.config_file_path.clone(),
                    cli_options.devices_file_path.clone(),
                )?;

            if !devices_file_exists {
                anyhow::bail!("No devices file found. Please add a device first.");
            }

            let nl_device = if config_file_exists {
                let config = config::Config::parse_from_file(&config_file_path)?;
                let name_to_search = if cli_options.device_name.is_some() {
                    &cli_options.device_name
                } else {
                    &config.default_nl_device_name
                };
                nanoleaf::NlDevice::find_in_file(&devices_file_path, name_to_search.as_deref())?
            } else {
                nanoleaf::NlDevice::find_in_file(
                    &devices_file_path,
                    cli_options.device_name.as_deref(),
                )?
            };

            let layout = nl_device.get_panel_layout()?;
            let orientation = nl_device.get_global_orientation()?;
            let global_orientation = orientation["value"].as_u64().unwrap_or(0) as u16;

            let panels = layout_visualizer::parse_layout(&layout)?;

            // Call the graphical visualizer - it has its own macroquad::main wrapper
            graphical_layout::visualize_graphical(panels, global_orientation, nl_device);

            Ok(())
        }
        config::DumpType::Info => {
            // Need to connect to device for info
            let ((config_file_path, config_file_exists), (devices_file_path, devices_file_exists)) =
                config::resolve_paths(
                    cli_options.config_file_path.clone(),
                    cli_options.devices_file_path.clone(),
                )?;

            if !devices_file_exists {
                anyhow::bail!("No devices file found. Please add a device first.");
            }

            let nl_device = if config_file_exists {
                let config = config::Config::parse_from_file(&config_file_path)?;
                let name_to_search = if cli_options.device_name.is_some() {
                    &cli_options.device_name
                } else {
                    &config.default_nl_device_name
                };
                nanoleaf::NlDevice::find_in_file(&devices_file_path, name_to_search.as_deref())?
            } else {
                nanoleaf::NlDevice::find_in_file(
                    &devices_file_path,
                    cli_options.device_name.as_deref(),
                )?
            };

            println!("Device Information for: {}", nl_device.name);
            println!("Device IP: {}", nl_device.ip);
            println!("\n=== Device Info (from /api/v1/) ===");
            let info = nl_device.get_device_info()?;
            println!("{}", serde_json::to_string_pretty(&info)?);

            Ok(())
        }
    }
}
