use crate::{
    audio::AudioStream,
    config::{Effect, VisualizerConfig},
    constants,
    nanoleaf::{self, NlDevice, NlUdp},
    panic, processing, utils,
};
use anyhow::Result;
use cpal::{InputCallbackInfo, SampleFormat, SizedSample, traits::*};
use dasp_sample::conv::ToSample;
use palette::Oklch;
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
    SetPalette(Vec<[u8; 3]>),
    SetEffect(Effect),
    ResetPanels,
    SetSorting {
        primary_axis: crate::config::Axis,
        sort_primary: crate::config::Sort,
        sort_secondary: crate::config::Sort,
        global_orientation: u16,
    },
}

pub struct Visualizer {
    state: VisualizerState,
    nl_device: NlDevice,
    nl_udp: NlUdp,
    audio_stream: AudioStream,
    gain: f32,
    time_window: f32,
    trans_time: u16,
    min_freq: u16,
    max_freq: u16,
    hues: Vec<[u8; 3]>,
    effect: Effect,
}

impl Visualizer {
    /// Initializes a new `Visualizer` instance with configuration and resources.
    ///
    /// Sets up UDP controller for panel updates, fetches global orientation from device for sorting.
    /// Applies config values or defaults for gain, time_window, etc.
    /// Sorts panels according to primary/secondary axes and sorts.
    ///
    /// Note: Audio stream is moved in, but actually played in `init()`.
    /// Does not start audio capture or visualization loop yet.
    ///
    /// # Arguments
    ///
    /// * `config` - VisualizerConfig with params like freq_range, hues, gain.
    /// * `audio_stream` - Pre-configured CPAL input stream device/config.
    /// * `nl_device` - Connected Nanoleaf device for UDP and API calls.
    ///
    /// # Errors
    ///
    /// From `NlUdp::new` if UDP setup fails, or device API for orientation.
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
        let hues = config
            .colors
            .unwrap_or(Vec::from(constants::DEFAULT_COLORS));
        let effect = config.effect.unwrap_or_default();
        Ok(Visualizer {
            state,
            nl_device: nl_device.clone(),
            nl_udp,
            audio_stream,
            gain,
            time_window,
            trans_time,
            min_freq,
            max_freq,
            hues,
            effect,
        })
    }

    /// Sends a UDP frame setting all panels to black with instant transition.
    ///
    /// Used when starting the visualizer or changing effects/palettes to clear
    /// any lingering colors from the previous state before the new effect begins.
    fn send_black_frame(&self, n_panels: usize) {
        let black = vec![Oklch::new(0.0, 0.0, 0.0); n_panels];
        let _ = self.nl_udp.update_panels(&black, 0);
    }

    /// Updates visualizer internal state or parameters based on received message.
    ///
    /// Dispatched in processing thread loop.
    /// Modifies self fields and/or regenerates `base_colors` vector from hues.
    /// When palette or sorting changes, brightness is reset to 0 so panels start dark.
    /// For SetSorting, updates UDP panel order with orientation.
    ///
    /// # Arguments
    ///
    /// * `event` - Control message type.
    /// * `base_colors` - Mutable reference to base Oklch colors with original lightness (updated if palette/sort changes).
    /// * `brightness` - Mutable reference to per-panel brightness multipliers (reset on palette/sort changes).
    /// * `prev_max` - Mutable reference to previous max amplitudes (reset on effect change).
    /// * `speed` - Mutable reference to velocity/phase accumulators (reset on effect change).
    fn update_state(
        &mut self,
        event: VisualizerMsg,
        base_colors: &mut Vec<Oklch>,
        brightness: &mut Vec<f32>,
        prev_max: &mut [f32],
        speed: &mut [f32],
    ) {
        match event {
            VisualizerMsg::Resume => self.state = VisualizerState::Running,
            VisualizerMsg::Pause => self.state = VisualizerState::Paused,
            VisualizerMsg::End => self.state = VisualizerState::Done,
            VisualizerMsg::SetGain(gain) => self.gain = gain,
            VisualizerMsg::SetEffect(effect) => {
                self.effect = effect;
                brightness.fill(0.0);
                // Reset state arrays — each effect uses prev_max/speed differently
                prev_max.fill(0.0);
                speed.fill(0.0);
                // Immediately send a black frame so the old effect's colors don't linger
                self.send_black_frame(base_colors.len());
            }
            VisualizerMsg::SetPalette(new_colors) => {
                self.hues = new_colors;
                *base_colors = utils::colors_from_rgb(&self.hues, base_colors.len());
                brightness.fill(0.0);
                // Immediately send a black frame so the old palette's colors don't linger
                self.send_black_frame(base_colors.len());
            }
            VisualizerMsg::ResetPanels => {
                brightness.fill(0.0);
                prev_max.fill(0.0);
                speed.fill(0.0);
                self.send_black_frame(base_colors.len());
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
                *base_colors = utils::colors_from_rgb(&self.hues, self.nl_udp.panels.len());
                brightness.resize(base_colors.len(), 0.0);
                brightness.fill(0.0);
                // Immediately send a black frame so the old sort order's colors don't linger
                self.send_black_frame(base_colors.len());
            }
        }
    }

    /// Converts interleaved multi-channel audio data to mono envelope (max per frame) and sends to channel.
    ///
    /// Processes audio callback data: for each set of `n_channels` samples, converts to f32,
    /// takes maximum absolute value as simplified mono amplitude envelope.
    /// Sends Vec<f32> of these envelopes to mpsc channel for FFT processing.
    ///
    /// Generic over sample type T supporting sized conversion to f32.
    /// Used in `create_data_callback` closure for CPAL stream.
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

    /// Creates a closure suitable for CPAL `build_input_stream` callback.
    ///
    /// Captures `n_channels` and `tx` sender, returns `send_samples` bound to them.
    /// The closure ignores `InputCallbackInfo` (timestamp not used).
    /// Ensures Send + 'static for thread-safe stream usage.
    ///
    /// Generic T for sample type matching AudioStream format.
    fn create_data_callback<T>(
        n_channels: usize,
        tx: mpsc::Sender<Vec<f32>>,
    ) -> impl FnMut(&[T], &InputCallbackInfo) + Send + 'static
    where
        T: SizedSample + ToSample<f32>,
    {
        move |data: &[T], _: &InputCallbackInfo| Self::send_samples(data, n_channels, &tx)
    }

    /// Completes visualizer setup by starting audio capture stream and spawning processing thread.
    ///
    /// Builds and plays CPAL input stream matched to sample format, sending mono max samples via channel.
    /// Spawns thread that:
    /// - Registers panic handler.
    /// - Loops receiving audio samples and control messages.
    /// - Processes FFT spectrum, updates per-panel brightness multiplier [0,1] from audio.
    /// - Computes display colors: for each panel, scales base Oklch lightness by brightness,
    ///   so at peak audio the output exactly matches the user's original RGB palette color.
    /// - Sends display colors to panels via UDP with transition time.
    /// - Handles pause/resume/end states.
    ///
    /// Returns sender for sending `VisualizerMsg` to control runtime behavior.
    ///
    /// Consumes self (moved into thread closure).
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
            let sample_rate = self.audio_stream.stream_config.sample_rate;
            // Base colors hold the target Oklch values (with original lightness from the user's RGB)
            let mut base_colors = utils::colors_from_rgb(&self.hues, n);
            // Brightness multiplier [0,1] per panel — animated by audio amplitude
            // At 0 the panel is black; at 1 it shows the exact target color
            let mut brightness = vec![0.0_f32; n];
            let mut prev_max = vec![0.0; n];
            let mut speed = vec![0.0; n];
            // Clear any colors left over from a previous Nanoleaf scene or effect
            self.send_black_frame(n);
            loop {
                match self.state {
                    VisualizerState::Done => break,
                    VisualizerState::Paused => {
                        let event = rx_events.recv().expect("events sender disconnected");
                        self.update_state(
                            event,
                            &mut base_colors,
                            &mut brightness,
                            &mut prev_max,
                            &mut speed,
                        );
                    }
                    VisualizerState::Running => match rx_events.try_recv() {
                        Ok(event) => self.update_state(
                            event,
                            &mut base_colors,
                            &mut brightness,
                            &mut prev_max,
                            &mut speed,
                        ),
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
                match self.effect {
                    Effect::Spectrum => processing::update_brightness(
                        spectrum,
                        hz_per_bin,
                        self.min_freq,
                        self.max_freq,
                        &mut brightness,
                        &mut prev_max,
                        &mut speed,
                    ),
                    Effect::EnergyWave => processing::update_brightness_wave(
                        spectrum,
                        hz_per_bin,
                        self.min_freq,
                        self.max_freq,
                        &mut brightness,
                        &mut prev_max,
                        &mut speed,
                    ),
                    Effect::Pulse => processing::update_brightness_pulse(
                        spectrum,
                        hz_per_bin,
                        self.min_freq,
                        self.max_freq,
                        &mut brightness,
                        &mut prev_max,
                        &mut speed,
                    ),
                }
                // Compute display colors: scale base lightness by brightness multiplier
                // This ensures at brightness=1.0, the output exactly matches the user's original RGB
                let display_colors: Vec<Oklch> = base_colors
                    .iter()
                    .zip(brightness.iter())
                    .map(|(base, &b)| Oklch::new(base.l * b, base.chroma, base.hue))
                    .collect();
                if self
                    .nl_udp
                    .update_panels(&display_colors, self.trans_time)
                    .is_err()
                {
                    // UDP send failed (e.g. extControl timed out) — re-request and retry once
                    if self.nl_device.request_udp_control().is_ok() {
                        let _ = self.nl_udp.update_panels(&display_colors, self.trans_time);
                    }
                }
            }
        });

        tx_events
    }
}
