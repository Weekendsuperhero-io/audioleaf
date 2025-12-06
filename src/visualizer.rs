use crate::{
    audio::AudioStream,
    config::VisualizerConfig,
    constants,
    nanoleaf::{self, NlDevice, NlUdp},
    panic, processing, utils,
};
use anyhow::Result;
use cpal::{traits::*, InputCallbackInfo, SampleFormat, SizedSample};
use dasp_sample::conv::ToSample;
use std::{
    sync::mpsc::{self, TryRecvError},
    thread,
};

#[derive(Debug, Default)]
enum VisualizerState {
    #[default]
    Paused,
    Running,
    Done,
}

#[derive(Debug)]
pub enum VisualizerMsg {
    Pause,
    Resume,
    End,
    SetGain(f32),
    SetPalette(Vec<u16>),
    SetSorting {
        primary_axis: crate::config::Axis,
        sort_primary: crate::config::Sort,
        sort_secondary: crate::config::Sort,
        global_orientation: u16,
    },
}

pub struct Visualizer {
    state: VisualizerState,
    nl_udp: NlUdp,
    audio_stream: AudioStream,
    gain: f32,
    time_window: f32,
    trans_time: i16,
    min_freq: u16,
    max_freq: u16,
    hues: Vec<u16>,
}

impl Visualizer {
    pub fn new(
        config: VisualizerConfig,
        audio_stream: AudioStream,
        nl_device: &NlDevice,
    ) -> Result<Self> {
        let state = VisualizerState::default();
        let mut nl_udp = nanoleaf::NlUdp::new(nl_device)?;

        // Get global orientation and apply it when sorting panels
        let global_orientation = nl_device
            .get_global_orientation()
            .ok()
            .and_then(|o| o["value"].as_u64())
            .unwrap_or(0) as u16;

        nl_udp.sort_panels_with_orientation(
            config.primary_axis,
            config.sort_primary,
            config.sort_secondary,
            global_orientation,
        );
        let gain = config.default_gain.unwrap_or(constants::DEFAULT_GAIN);
        let time_window = config.time_window.unwrap_or(constants::DEFAULT_TIME_WINDOW);
        let trans_time = config
            .transition_time
            .unwrap_or(constants::DEFAULT_TRANSITION_TIME);
        let (min_freq, max_freq) = config.freq_range.unwrap_or(constants::DEFAULT_FREQ_RANGE);
        let hues = config.hues.unwrap_or(Vec::from(constants::DEFAULT_HUES));
        Ok(Visualizer {
            state,
            nl_udp,
            audio_stream,
            gain,
            time_window,
            trans_time,
            min_freq,
            max_freq,
            hues,
        })
    }

    fn update_state(&mut self, event: VisualizerMsg, colors: &mut Vec<palette::Hwb>) {
        match event {
            VisualizerMsg::Resume => self.state = VisualizerState::Running,
            VisualizerMsg::Pause => self.state = VisualizerState::Paused,
            VisualizerMsg::End => self.state = VisualizerState::Done,
            VisualizerMsg::SetGain(gain) => self.gain = gain,
            VisualizerMsg::SetPalette(hues) => {
                self.hues = hues;
                *colors = utils::colors_from_hues(&self.hues, colors.len());
            }
            VisualizerMsg::SetSorting {
                primary_axis,
                sort_primary,
                sort_secondary,
                global_orientation,
            } => {
                self.nl_udp.sort_panels_with_orientation(
                    Some(primary_axis),
                    Some(sort_primary),
                    Some(sort_secondary),
                    global_orientation,
                );
                *colors = utils::colors_from_hues(&self.hues, self.nl_udp.panels.len());
            }
        }
    }

    fn send_samples<T>(data: &[T], n_channels: usize, tx: &mpsc::Sender<Vec<f32>>)
    where
        T: SizedSample + ToSample<f32>,
    {
        let mut samples = Vec::with_capacity(data.len());
        for chunk in data.chunks_exact(n_channels) {
            samples.push(
                chunk
                    .iter()
                    .map(|x| x.to_sample::<f32>())
                    .fold(f32::NEG_INFINITY, f32::max),
            );
        }
        tx.send(samples).expect("sending samples failed");
    }

    fn create_data_callback<T>(
        n_channels: usize,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> impl FnMut(&[T], &InputCallbackInfo) + Send + 'static
    where
        T: SizedSample + ToSample<f32>,
    {
        move |data: &[T], _: &InputCallbackInfo| Self::send_samples(data, n_channels, &tx)
    }

    pub fn init(mut self) -> mpsc::Sender<VisualizerMsg> {
        let (tx_events, rx_events) = mpsc::channel();
        thread::spawn(move || {
            panic::register_backtrace_panic_handler();
            let (tx_audio, rx_audio) = mpsc::channel();
            macro_rules! build_input_stream {
                ($type:ty) => {
                    self.audio_stream
                        .device
                        .build_input_stream(
                            &self.audio_stream.stream_config,
                            Self::create_data_callback::<$type>(
                                self.audio_stream.stream_config.channels as usize,
                                tx_audio,
                            ),
                            move |_| panic!("building the audio stream failed"),
                            None,
                        )
                        .expect("stream initialization failed")
                };
            }
            let stream = match self.audio_stream.sample_format {
                SampleFormat::I8 => build_input_stream!(i8),
                SampleFormat::I16 => build_input_stream!(i16),
                SampleFormat::I32 => build_input_stream!(i32),
                SampleFormat::I64 => build_input_stream!(i64),
                SampleFormat::U8 => build_input_stream!(u8),
                SampleFormat::U16 => build_input_stream!(u16),
                SampleFormat::U32 => build_input_stream!(u32),
                SampleFormat::U64 => build_input_stream!(u64),
                SampleFormat::F32 => build_input_stream!(f32),
                SampleFormat::F64 => build_input_stream!(f64),
                _ => panic!("unsupported sample format"),
            };
            stream.play().expect("running the audio stream failed");

            let n = self.nl_udp.panels.len();
            let sample_rate = self.audio_stream.stream_config.sample_rate.0;
            let mut colors = utils::colors_from_hues(&self.hues, n);
            let mut prev_max = vec![0.0; n];
            let mut speed = vec![0.0; n];
            loop {
                match self.state {
                    VisualizerState::Done => break,
                    VisualizerState::Paused => {
                        let event = rx_events.recv().expect("events sender disconnected");
                        self.update_state(event, &mut colors);
                    }
                    VisualizerState::Running => match rx_events.try_recv() {
                        Ok(event) => self.update_state(event, &mut colors),
                        Err(err) => {
                            if err == TryRecvError::Disconnected {
                                panic!("events sender disconnected");
                            }
                        }
                    },
                }
                let to_collect = ((sample_rate as f32) * self.time_window).round() as usize;
                let mut samples = Vec::with_capacity(2 * to_collect);
                while samples.len() < to_collect {
                    let mut new_samples = rx_audio.recv().expect("receiving samples failed");
                    samples.append(&mut new_samples);
                }
                let spectrum = processing::process(samples, self.gain);
                let hz_per_bin = (sample_rate / 2) / (spectrum.len() as u32);
                processing::update_colors(
                    spectrum,
                    hz_per_bin,
                    self.min_freq,
                    self.max_freq,
                    &mut colors,
                    &mut prev_max,
                    &mut speed,
                );
                self.nl_udp
                    .update_panels(&colors, self.trans_time)
                    .expect("updating panels failed");
            }
        });

        tx_events
    }
}
