use crate::config::Config;
use crate::nanoleaf::{Command, NanoleafDevice, Panel};
use crate::{audio, constants};
use cli_log::*;
use cpal::{
    traits::*, Device, InputCallbackInfo, Sample, SampleFormat, StreamConfig, SupportedStreamConfig,
};
use palette::{FromColor, Hwb, Srgb};
use std::net::UdpSocket;
use std::sync::mpsc;
use std::thread;

#[derive(Debug)]
pub enum VisualizerEvent {
    Pause,
    End,
    Resume(UdpSocket), // fix this absolute garbage lol
}

pub fn run_commands(
    commands: Vec<Command>,
    panels: &[Panel],
    udp_socket: &UdpSocket,
) -> Result<(), anyhow::Error> {
    let split_into_bytes = |x: u16| -> (u8, u8) {
        // split a u16 into two bytes (in big endian), e.g. 651 -> (2, 139) because 651 = 2 * 256 + 139
        ((x / 256) as u8, (x % 256) as u8)
    };

    let n_panels = commands.len();
    let mut buf = vec![0; 2];
    (buf[0], buf[1]) = split_into_bytes(n_panels as u16);
    for command in commands.iter() {
        let Command {
            panel_no,
            color: color_hwb,
            transition_time,
        } = command;
        let color_rgb = Srgb::from_color(*color_hwb).into_format::<u8>();
        let Srgb {
            red,
            green,
            blue,
            standard: _,
        } = color_rgb;

        let mut sub_buf = [0u8; 8];
        (sub_buf[0], sub_buf[1]) = split_into_bytes(panels[*panel_no - 1].id);
        (sub_buf[2], sub_buf[3], sub_buf[4], sub_buf[5]) = (red, green, blue, 0);
        (sub_buf[6], sub_buf[7]) = split_into_bytes(*transition_time);
        buf.extend(sub_buf);
    }
    udp_socket.send(&buf)?;

    Ok(())
}

fn data_callback(data: Vec<f32>, n_channels: usize, tx: &mpsc::Sender<Vec<f32>>) {
    let mut samples = Vec::new(); // change this to with_capacity
    debug!("{}", data.len());
    for chunk in data.chunks_exact(n_channels) {
        // max of samples from all channels
        samples.push(
            chunk
                .iter()
                .fold(f32::NEG_INFINITY, |acc, x| f32::max(acc, *x)),
        );
    }
    tx.send(samples).unwrap();
}

pub fn setup_visualizer_thread(
    device: Device,
    sample_format: SampleFormat,
    stream_config: StreamConfig,
    config: &Config,
    panels: Vec<Panel>,
) -> Result<(thread::JoinHandle<impl Send>, mpsc::Sender<VisualizerEvent>), anyhow::Error> {
    let (transition_time, min_freq, max_freq, mut boost) = (
        config.transition_time,
        config.min_freq,
        config.max_freq,
        config.default_boost,
    );
    let mut colors = config
        .hues
        .iter()
        .map(|hue| palette::Hwb::new(*hue as f32, 1.0, 0.0))
        .collect::<Vec<_>>();
    let active_panels_numbers = config.active_panels_numbers.clone();
    let sample_rate = stream_config.sample_rate.0;
    let (tx_events, rx_events) = mpsc::channel();
    let visualizer_thread = thread::spawn(move || {
        let (tx_audio, rx_audio) = mpsc::channel();
        let error_callback = move |err| warn!("{}", err); // TODO: replace this with a write to some log file
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data, _: &InputCallbackInfo| {
                        data_callback(data.to_vec(), stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::F64 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f64], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::I8 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i8], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::I32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i32], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::I64 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i64], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::U8 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u8], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::U16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u16], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::U32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u32], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            cpal::SampleFormat::U64 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u64], _: &InputCallbackInfo| {
                        let data_f32 =
                            Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                        data_callback(data_f32, stream_config.channels as usize, &tx_audio)
                    },
                    error_callback,
                    None,
                )
                .unwrap(),
            // sample_format => {
            //     return Err(anyhow::Error::msg(format!(
            //         "Unsupported sample format: {}",
            //         sample_format
            //     )));
            // }
            _ => {
                return;
            }
        };
        let _ = stream.play();

        let (mut pause, mut end) = (true, false);
        let mut udp_socket = None;
        loop {
            if let Ok(event) = rx_events.try_recv() {
                match event {
                    VisualizerEvent::Pause => pause = true,
                    VisualizerEvent::End => end = true,
                    _ => (),
                }
            }
            if pause {
                loop {
                    if let Ok(event) = rx_events.recv() {
                        match event {
                            VisualizerEvent::Resume(socket) => {
                                pause = false;
                                udp_socket = Some(socket);
                                break;
                            }
                            VisualizerEvent::End => {
                                end = true;
                                break;
                            }
                            _ => (),
                        }
                    }
                }
            }
            if end {
                break;
            }

            let mut samples = Vec::with_capacity(sample_rate as usize);
            while samples.len() < (sample_rate as usize) / 10 { // make this a configurable parameter, 10 is a nice default (maybe try 8)
                if let Ok(mut new_samples) = rx_audio.recv() {
                    samples.append(&mut new_samples);
                }
            }
            let freq_spectrum = audio::process(samples, boost);
            let hz_per_bin = (sample_rate / 2) / (freq_spectrum.len() as u32);
            audio::update_colors(&mut colors, freq_spectrum, min_freq, max_freq, hz_per_bin);
            let commands = active_panels_numbers
                .iter()
                .zip(colors.iter())
                .map(|(panel_no, color)| Command {
                    panel_no: *panel_no as usize,
                    color: *color,
                    transition_time,
                })
                .collect::<Vec<_>>();
            if let Some(udp_socket) = &udp_socket {
                if run_commands(commands, &panels, udp_socket).is_err() {
                    end = true;
                }
            }
        }
    });

    Ok((visualizer_thread, tx_events))
}

pub fn setup_audio_device(
    device_name: &str,
) -> Result<(Device, SampleFormat, StreamConfig), anyhow::Error> {
    let host = cpal::default_host();
    let device = match device_name {
        constants::DEFAULT_AUDIO_DEVICE => host.default_input_device(),
        _ => host
            .input_devices()?
            .find(|x| x.name().map(|y| y == device_name).unwrap_or(false)),
    };
    let device = if let Some(device) = device {
        device
    } else {
        return Err(anyhow::Error::msg(format!(
            "Input device \"{}\" not found, available input devices are: {}",
            device_name,
            host.input_devices()?.fold(String::new(), |acc, device| acc
                + &device.name().unwrap_or_default()
                + ", ")
        )));
    };
    let audio_config = device.default_input_config()?;
    let sample_format = audio_config.sample_format();
    let stream_config: StreamConfig = audio_config.into();

    Ok((device, sample_format, stream_config))
}
