use crate::constants;
use anyhow::{anyhow, Result};
use cpal::{traits::*, Device, SampleFormat, StreamConfig};

pub struct AudioStream {
    pub device: Device,
    pub sample_format: SampleFormat,
    pub stream_config: StreamConfig,
}

impl AudioStream {
    pub fn new(device_name: Option<&str>) -> Result<Self> {
        let device_name = match device_name {
            Some(name) => name,
            None => constants::DEFAULT_AUDIO_BACKEND,
        };
        let host = cpal::default_host();
        let device = match device_name {
            constants::DEFAULT_AUDIO_BACKEND => host.default_input_device(),
            _ => host
                .input_devices()?
                .find(|x| x.name().map(|y| y == device_name).unwrap_or(false)),
        };
        let Some(device) = device else {
            return Err(anyhow!(format!(
                "Audio backend `{}` not found, available options: {}",
                device_name,
                host.input_devices()?
                    .map(|dev| dev.name().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
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
