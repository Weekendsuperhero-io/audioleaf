use crate::constants;
use anyhow::{Result, bail};
use cpal::{Device, SampleFormat, StreamConfig, traits::*};
use std::collections::HashMap;

pub struct AudioStream {
    pub device: Device,
    pub sample_format: SampleFormat,
    pub stream_config: StreamConfig,
}

impl AudioStream {
    /// Creates a new `AudioStream` instance for capturing audio from an input device.
    ///
    /// Prioritizes loopback/monitor devices for system-wide audio capture (e.g., BlackHole on macOS).
    /// Searches common names like "BlackHole", "Loopback Audio", etc.
    /// Falls back to the system's default input device (often microphone) with a warning.
    /// Supports specifying a custom device name via `device_name`.
    ///
    /// # Arguments
    ///
    /// * `device_name` - Optional name of the audio device to use. Defaults to automatic loopback detection.
    ///
    /// # Returns
    ///
    /// `Result<Self>` with the configured `AudioStream` containing device, sample format, and config.
    ///
    /// # Errors
    ///
    /// Propagates `cpal` errors for device discovery or config retrieval. Bail with available devices list if none match.
    pub fn new(device_name: Option<&str>) -> Result<Self> {
        let device_name = match device_name {
            Some(name) => name,
            None => constants::DEFAULT_AUDIO_BACKEND,
        };
        let host = cpal::default_host();
        let input_devices = enumerate_input_devices(&host)?;

        // Try to find the device in input devices (for loopback/monitor devices)
        let device = match device_name {
            constants::DEFAULT_AUDIO_BACKEND => {
                // Check for common loopback device names first
                let loopback_names = [
                    "BlackHole",
                    "BlackHole 2ch",
                    "BlackHole 16ch",
                    "Loopback Audio",
                    "CABLE Output",
                    "VB-Audio",
                    "Monitor",
                    "monitor",
                ];

                let mut loopback_device = None;
                for (device, raw_name, _) in &input_devices {
                    if loopback_names.iter().any(|lb| raw_name.contains(lb)) {
                        #[cfg(debug_assertions)]
                        eprintln!("INFO: Found loopback device: {}", raw_name);
                        loopback_device = Some(device.clone());
                        break;
                    }
                }

                // Fall back to default input if no loopback found
                loopback_device.or_else(|| {
                    eprintln!("WARNING: No loopback device found. Using default input device (microphone).");
                    eprintln!("To capture system audio on macOS, install BlackHole: https://github.com/ExistentialAudio/BlackHole");
                    host.default_input_device()
                })
            }
            _ => select_input_device(&input_devices, device_name).map(|(device, _, _)| device),
        };

        let Some(device) = device else {
            bail!(format!(
                "Audio backend `{}` not found, available options: {}",
                device_name,
                list_input_backend_names()?.join(", ")
            ));
        };
        let audio_config = device.default_input_config()?;
        let sample_format = audio_config.sample_format();
        let stream_config: StreamConfig = audio_config.into();

        Ok(AudioStream {
            device,
            sample_format,
            stream_config,
        })
    }
}

/// Returns input backend names with stable disambiguation suffixes for duplicates.
///
/// Example:
/// - "Loopback, Loopback PCM [#1]"
/// - "Loopback, Loopback PCM [#2]"
pub fn list_input_backend_names() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let devices = enumerate_input_devices(&host)?;
    Ok(devices.into_iter().map(|(_, _, display)| display).collect())
}

fn enumerate_input_devices(host: &cpal::Host) -> Result<Vec<(Device, String, String)>> {
    let raw_devices: Vec<(Device, String)> = host
        .input_devices()?
        .filter_map(|device| {
            device
                .description()
                .ok()
                .map(|description| (device, description.name().to_string()))
        })
        .collect();

    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, raw_name) in &raw_devices {
        *counts.entry(raw_name.clone()).or_default() += 1;
    }

    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut result = Vec::with_capacity(raw_devices.len());
    for (device, raw_name) in raw_devices {
        let total = counts.get(&raw_name).copied().unwrap_or(1);
        let display_name = if total > 1 {
            let next = seen.entry(raw_name.clone()).or_default();
            *next += 1;
            format!("{} [#{}]", raw_name, *next)
        } else {
            raw_name.clone()
        };
        result.push((device, raw_name, display_name));
    }

    Ok(result)
}

fn select_input_device(
    devices: &[(Device, String, String)],
    selected_name: &str,
) -> Option<(Device, String, String)> {
    if let Some((base_name, duplicate_index)) = parse_indexed_name(selected_name) {
        let mut matched_index = 0usize;
        for (device, raw_name, display_name) in devices {
            if raw_name == &base_name {
                matched_index += 1;
                if matched_index == duplicate_index {
                    return Some((device.clone(), raw_name.clone(), display_name.clone()));
                }
            }
        }
        return None;
    }

    devices
        .iter()
        .find(|(_, raw_name, display_name)| {
            raw_name == selected_name || display_name == selected_name
        })
        .map(|(device, raw_name, display_name)| {
            (device.clone(), raw_name.clone(), display_name.clone())
        })
}

fn parse_indexed_name(name: &str) -> Option<(String, usize)> {
    if !name.ends_with(']') {
        return None;
    }
    let marker_start = name.rfind(" [#")?;
    let index_part = &name[(marker_start + 3)..(name.len() - 1)];
    let parsed_index = index_part.parse::<usize>().ok()?;
    if parsed_index == 0 {
        return None;
    }
    let base_name = name[..marker_start].to_string();
    if base_name.is_empty() {
        return None;
    }
    Some((base_name, parsed_index))
}
