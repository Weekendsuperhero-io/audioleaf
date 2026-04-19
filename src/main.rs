use anyhow::Result;
use clap::Parser;

mod audio;
mod config;
mod constants;
mod layout_visualizer;
mod nanoleaf;
mod now_playing;
mod palettes;
mod panic;
mod processing;
mod ssdp;
mod utils;
mod visualizer;

fn main() -> Result<()> {
    panic::register_backtrace_panic_handler();
    let cli_options = config::CliOptions::parse();

    if let Some(config::Command::Dump { dump_type }) = &cli_options.command {
        return handle_dump_command(dump_type, &cli_options);
    }

    // Normal mode: print instructions and open the web UI
    println!("Audioleaf v{}", env!("CARGO_PKG_VERSION"));
    println!("Start the API server with: audioleaf-api");
    println!("Then open http://localhost:8787 in your browser.");
    Ok(())
}

fn handle_dump_command(
    dump_type: &config::DumpType,
    cli_options: &config::CliOptions,
) -> Result<()> {
    match dump_type {
        config::DumpType::Layout => {
            let (config_file_path, devices_file_path) = resolve_device_paths(cli_options)?;
            let nl_device = resolve_device(cli_options, &config_file_path, &devices_file_path)?;

            println!("Panel Layout Information for: {}", nl_device.name);
            println!("Device IP: {}", nl_device.ip);
            nl_device.ensure_device_ready()?;

            let layout = nl_device.get_panel_layout()?;
            let orientation = nl_device.get_global_orientation()?;
            let global_orientation = orientation["value"].as_u64().unwrap_or(0) as u16;
            let panels = layout_visualizer::parse_layout(&layout)?;
            layout_visualizer::visualize_layout(&panels, global_orientation);

            println!("\n=== Raw Panel Layout JSON ===");
            println!("{}", serde_json::to_string_pretty(&layout)?);
            println!("\n=== Raw Global Orientation JSON ===");
            println!("{}", serde_json::to_string_pretty(&orientation)?);

            nl_device.set_state(Some(false), Some(0))?;
            Ok(())
        }
        config::DumpType::Palettes => {
            println!("Available Color Palettes:\n");
            let mut palette_names = palettes::get_palette_names();
            palette_names.sort();
            for name in palette_names {
                let colors = palettes::get_palette(&name).unwrap();
                let color_strs: Vec<String> = colors
                    .iter()
                    .map(|[r, g, b]| format!("[{}, {}, {}]", r, g, b))
                    .collect();
                println!("  {} = [{}]", name, color_strs.join(", "));
            }
            Ok(())
        }
        config::DumpType::Info => {
            let (config_file_path, devices_file_path) = resolve_device_paths(cli_options)?;
            let nl_device = resolve_device(cli_options, &config_file_path, &devices_file_path)?;

            println!("Device Information for: {}", nl_device.name);
            println!("Device IP: {}", nl_device.ip);
            nl_device.ensure_device_ready()?;

            println!("\n=== Device Info (from /api/v1/) ===");
            let info = nl_device.get_device_info()?;
            println!("{}", serde_json::to_string_pretty(&info)?);

            nl_device.set_state(Some(false), Some(0))?;
            Ok(())
        }
    }
}

fn resolve_device_paths(
    cli_options: &config::CliOptions,
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let ((config_path, _), (devices_path, devices_exists)) = config::resolve_paths(
        cli_options.config_file_path.clone(),
        cli_options.devices_file_path.clone(),
    )?;
    if !devices_exists {
        anyhow::bail!("No devices file found. Please add a device first.");
    }
    Ok((config_path, devices_path))
}

fn resolve_device(
    cli_options: &config::CliOptions,
    config_file_path: &std::path::Path,
    devices_file_path: &std::path::Path,
) -> Result<nanoleaf::NlDevice> {
    if config_file_path.exists() {
        let config = config::Config::parse_from_file(config_file_path)?;
        let name = if cli_options.device_name.is_some() {
            &cli_options.device_name
        } else {
            &config.default_nl_device_name
        };
        nanoleaf::NlDevice::find_in_file(devices_file_path, name.as_deref())
    } else {
        nanoleaf::NlDevice::find_in_file(devices_file_path, cli_options.device_name.as_deref())
    }
}
