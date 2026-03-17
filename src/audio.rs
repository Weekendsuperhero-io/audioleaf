use crate::constants;
use anyhow::{Result, bail};
use cpal::{Device, SampleFormat, StreamConfig, traits::*};

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
                if let Ok(devices) = host.input_devices() {
                    for device in devices {
                        if let Ok(name) = device.description().map(|d| d.name().to_string())
                            && loopback_names.iter().any(|lb| name.contains(lb))
                        {
                            #[cfg(debug_assertions)]
                            eprintln!("INFO: Found loopback device: {}", name);
                            loopback_device = Some(device);
                            break;
                        }
                    }
                }

                // Fall back to default input if no loopback found
                loopback_device.or_else(|| {
                    eprintln!("WARNING: No loopback device found. Using default input device (microphone).");
                    eprintln!("To capture system audio on macOS, install BlackHole: https://github.com/ExistentialAudio/BlackHole");
                    host.default_input_device()
                })
            }
            _ => host.input_devices()?.find(|x| {
                x.description()
                    .map(|d| d.name() == device_name)
                    .unwrap_or(false)
            }),
        };

        let Some(device) = device else {
            bail!(format!(
                "Audio backend `{}` not found, available options: {}",
                device_name,
                host.input_devices()?
                    .map(|dev| dev
                        .description()
                        .map(|d| d.name().to_string())
                        .unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(", ")
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
