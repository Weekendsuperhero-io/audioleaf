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

/// The main entry point of the Audioleaf application.
///
/// This function blocks on the asynchronous main logic using `pollster::block_on`.
/// It sets up panic handling and parses CLI options before delegating to `main_async`.
fn main() -> Result<()> {
    pollster::block_on(main_async())
}

/// Asynchronous main logic of the Audioleaf application.
///
/// Parses CLI options and handles different modes:
/// - Dump commands (layout, palettes, info, graphical layout) without TUI.
/// - Normal mode: loads or discovers Nanoleaf device, sets up config, runs TUI visualizer/effect selector.
///
/// Ensures device is ready (powered on, brightness set) before running the app.
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

/// Handles 'dump' subcommands to display Nanoleaf device information or configuration without launching the TUI.
///
/// Supported dump types:
/// - `Layout`: Fetches and prints panel layout data and global orientation.
/// - `Palettes`: Lists all predefined color palettes available in the application.
/// - `LayoutGraphical`: Renders an interactive graphical visualization of the panel layout using macroquad.
/// - `Info`: Retrieves and prints basic device information from the /api/v1/ endpoint.
///
/// In all cases except `Palettes`, it connects to a known device or uses CLI-specified name.
///
/// # Arguments
///
/// * `dump_type` - Specifies which type of information to dump.
/// * `cli_options` - Parsed CLI options including config paths and device name.
///
/// # Errors
///
/// Returns `anyhow::Error` for issues like missing devices file, connection failures, or JSON parsing errors.
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
