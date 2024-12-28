use crate::audio;
use crate::config::Config;
use cpal::{
    traits::*, Device, InputCallbackInfo, Sample, SampleFormat, StreamConfig, SupportedStreamConfig,
};
use std::sync::mpsc;
use std::thread;

#[derive(Debug)]
pub enum Msg {
    Pause,
    End,
    Resume,
    Samples(Vec<f32>),
}

fn data_callback(data: Vec<f32>, n_channels: usize, tx: &mpsc::Sender<Msg>) {
    let mut samples = Vec::new();
    for chunk in data.chunks_exact(n_channels) {
        // max of samples from all channels
        samples.push(
            chunk
                .iter()
                .fold(f32::NEG_INFINITY, |acc, x| f32::max(acc, *x)),
        );
    }
    tx.send(Msg::Samples(samples)).unwrap();
}

pub fn setup_visualizer_thread(
    device: Device,
    sample_format: SampleFormat,
    stream_config: StreamConfig,
    config: &Config,
) -> Result<mpsc::Sender<Msg>, anyhow::Error> {
    let (tx, rx) = mpsc::channel();
    let tx2 = tx.clone();
    let error_callback = |_| {}; // TODO: replace this with a write to some log file
    let mut stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data, _: &InputCallbackInfo| {
                data_callback(data.to_vec(), stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::F64 => device.build_input_stream(
            &stream_config,
            move |data: &[f64], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::I8 => device.build_input_stream(
            &stream_config,
            move |data: &[i8], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::I32 => device.build_input_stream(
            &stream_config,
            move |data: &[i32], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::I64 => device.build_input_stream(
            &stream_config,
            move |data: &[i64], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::U8 => device.build_input_stream(
            &stream_config,
            move |data: &[u8], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::U32 => device.build_input_stream(
            &stream_config,
            move |data: &[u32], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::U64 => device.build_input_stream(
            &stream_config,
            move |data: &[u64], _: &InputCallbackInfo| {
                let data_f32 = Vec::from_iter(data.iter().map(|sample| sample.to_sample::<f32>()));
                data_callback(data_f32, stream_config.channels as usize, &tx)
            },
            error_callback,
            None,
        )?,
        sample_format => {
            return Err(anyhow::Error::msg(format!(
                "Unsupported sample format: {}",
                sample_format
            )));
        }
    };

    let sample_rate = stream_config.sample_rate.0;
    let (transition_time, min_freq, max_freq, boost) = (
        config.transition_time,
        config.min_freq,
        config.max_freq,
        config.default_boost,
    );
    let hues = config.hues.clone();
    tx2.send(Msg::Pause)?;
    let visualizer_thread = thread::spawn(move || {
        let mut colors = hues
            .into_iter()
            .map(|hue| palette::Hwb::<f32>::new(hue as f32, 1.0, 0.0))
            .collect::<Vec<_>>();
        'main_loop: loop {
            let mut samples = Vec::<f32>::with_capacity(sample_rate as usize);
            while samples.len() < (sample_rate as usize) / 2 {
                match rx.recv().unwrap() {
                    Msg::Samples(new_samples) => samples.extend(new_samples),
                    // Msg::Pause => stream.pause().unwrap(), // this causes an issue
                    _ => todo!(),
                }
            }
        }
        // 'main_loop: loop {
        //     // needs to block here and wait for a Msg to come
        //     let mut samples = Vec::with_capacity(sample_rate as usize);
        //     while samples.len() < (sample_rate as usize) / 2 {
        //         match rx.recv().unwrap() {
        //             Msg::Samples(new_samples) => samples.extend(new_samples),
        //             Msg::Pause => {
        //                 stream.pause().unwrap();
        //             }
        //             Msg::Resume => {
        //                 stream.play().unwrap();
        //             }
        //             Msg::End => {
        //                 break 'main_loop;
        //             }
        //         }
        //     }
        //     let freq_spectrum = audio::process(samples, boost);
        //     let hz_per_bin = (sample_rate / 2) / (freq_spectrum.len() as u32);
        //     audio::update_colors(&mut colors, freq_spectrum, min_freq, max_freq, hz_per_bin);
        //     // let commands =
        // }
    });

    Ok(tx2)
}

pub fn setup_audio_device(
    device_name: &str,
) -> Result<(Device, SampleFormat, StreamConfig), anyhow::Error> {
    let host = cpal::default_host();
    let device = match device_name {
        "default" => host.default_input_device(),
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
