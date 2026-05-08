use crate::{
    audio::AudioStream,
    config::{Effect, VisualizerConfig},
    constants,
    nanoleaf::{self, NlDevice, NlUdp},
    panic, processing, utils,
};
use anyhow::Result;
use cpal::{InputCallbackInfo, SampleFormat, SizedSample, StreamError, traits::*};
use dasp_sample::conv::ToSample;
use flume::{self, RecvTimeoutError, TryRecvError};
use hashbrown::HashMap;
use palette::{FromColor, Oklch, Srgb};
use parking_lot::Mutex;
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const STREAM_ERROR_REPORT_INTERVAL: Duration = Duration::from_secs(10);
const AUDIO_RECV_TIMEOUT: Duration = Duration::from_millis(250);
/// Bound on the cpal-callback → visualizer audio queue. Capped so a producer
/// burst (e.g. AirPlay catching up after a stall) cannot grow memory
/// unboundedly; the callback drops oldest batches when the consumer falls
/// behind.
const AUDIO_TX_CHANNEL_DEPTH: usize = 64;
/// Initial wait before retrying audio-stream construction after a fault.
const AUDIO_RESTART_INITIAL_BACKOFF: Duration = Duration::from_millis(250);
/// Cap on the audio-stream rebuild backoff.
const AUDIO_RESTART_MAX_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
enum StreamFault {
    DeviceUnavailable,
}

/// Why the inner audio pump returned to the outer loop.
enum AudioLoopExit {
    /// Visualizer was asked to stop (End message). Exit the thread.
    Done,
    /// The audio stream went away or the device is unavailable. Drop the
    /// current cpal stream, sleep with backoff, and try to rebuild — without
    /// disturbing the Nanoleaf attachment.
    RebuildAudio,
}

#[derive(Debug, Clone, Copy)]
enum StreamErrorKind {
    PollErr,
    XRun,
    DeviceUnavailable,
    Other,
}

#[derive(Debug, Default, Clone, Copy)]
struct StreamErrorCounts {
    pollerr: u64,
    xrun: u64,
    other: u64,
    device_unavailable: u64,
}

impl StreamErrorCounts {
    fn total(self) -> u64 {
        self.pollerr + self.xrun + self.other + self.device_unavailable
    }
}

#[derive(Debug)]
struct StreamErrorTelemetry {
    interval: StreamErrorCounts,
    lifetime: StreamErrorCounts,
    last_report: Instant,
}

impl StreamErrorTelemetry {
    fn new() -> Self {
        Self {
            interval: StreamErrorCounts::default(),
            lifetime: StreamErrorCounts::default(),
            last_report: Instant::now(),
        }
    }

    fn record(&mut self, kind: StreamErrorKind) {
        let (interval_counter, lifetime_counter) = match kind {
            StreamErrorKind::PollErr => (&mut self.interval.pollerr, &mut self.lifetime.pollerr),
            StreamErrorKind::XRun => (&mut self.interval.xrun, &mut self.lifetime.xrun),
            StreamErrorKind::Other => (&mut self.interval.other, &mut self.lifetime.other),
            StreamErrorKind::DeviceUnavailable => (
                &mut self.interval.device_unavailable,
                &mut self.lifetime.device_unavailable,
            ),
        };
        *interval_counter = interval_counter.saturating_add(1);
        *lifetime_counter = lifetime_counter.saturating_add(1);
    }

    fn take_report_if_due(
        &mut self,
        now: Instant,
    ) -> Option<(StreamErrorCounts, StreamErrorCounts)> {
        if now.duration_since(self.last_report) < STREAM_ERROR_REPORT_INTERVAL {
            return None;
        }
        let interval = self.interval;
        let lifetime = self.lifetime;
        self.interval = StreamErrorCounts::default();
        self.last_report = now;
        Some((interval, lifetime))
    }
}

#[derive(Debug, Default)]
enum VisualizerState {
    #[default]
    Running,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamHealth {
    #[default]
    Starting,
    Healthy,
    Degraded,
    Restarting,
    Stopped,
}

#[derive(Debug, Clone)]
pub enum VisualizerMsg {
    End,
    Ping,
    SetGain(f32),
    SetPalette(Vec<[u8; 3]>),
    SetEffect(Effect),
    ResetPanels,
    SetTimeWindow(f32),
    SetTransitionTime(u16),
    SetFreqRange(u16, u16),
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
    color_subscribers: Vec<flume::Sender<HashMap<u16, [u8; 3]>>>,
    stream_health: Option<Arc<Mutex<StreamHealth>>>,
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
        color_subscribers: Vec<flume::Sender<HashMap<u16, [u8; 3]>>>,
        initial_hues: Vec<[u8; 3]>,
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
        // Caller resolves colors (from Nanoleaf palette, artwork, or fallback)
        // before constructing — this is just a safety net for empty input.
        let hues = if initial_hues.is_empty() {
            Vec::from(constants::DEFAULT_COLORS)
        } else {
            initial_hues
        };
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
            color_subscribers,
            stream_health: None,
        })
    }

    pub fn with_stream_health(mut self, stream_health: Arc<Mutex<StreamHealth>>) -> Self {
        self.stream_health = Some(stream_health);
        self
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
            VisualizerMsg::End => self.state = VisualizerState::Done,
            VisualizerMsg::Ping => {}
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
            VisualizerMsg::SetTimeWindow(tw) => self.time_window = tw,
            VisualizerMsg::SetTransitionTime(tt) => self.trans_time = tt,
            VisualizerMsg::SetFreqRange(min, max) => {
                self.min_freq = min;
                self.max_freq = max;
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
    fn send_samples<T>(data: &[T], n_channels: usize, tx: &flume::Sender<Vec<f32>>)
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
        // Bounded channel: if the consumer is falling behind, drop this
        // batch on the floor. cpal callbacks must never block, so send()
        // is unsafe; and the channel only exposes try_send from the
        // producer side. Dropping recent samples is acceptable — the
        // visualizer treats missing audio as silence.
        let _ = tx.try_send(samples);
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
        tx: flume::Sender<Vec<f32>>,
    ) -> impl FnMut(&[T], &InputCallbackInfo) + Send + 'static
    where
        T: SizedSample + ToSample<f32>,
    {
        move |data: &[T], _: &InputCallbackInfo| Self::send_samples(data, n_channels, &tx)
    }

    fn classify_stream_error(err: &StreamError, err_text: &str) -> StreamErrorKind {
        match err {
            StreamError::DeviceNotAvailable | StreamError::StreamInvalidated => {
                StreamErrorKind::DeviceUnavailable
            }
            StreamError::BufferUnderrun => StreamErrorKind::XRun,
            StreamError::BackendSpecific { .. } => {
                if err_text.contains("Buffer underrun/overrun occurred") {
                    StreamErrorKind::XRun
                } else if err_text.contains("POLLERR") {
                    StreamErrorKind::PollErr
                } else if err_text.contains("device is no longer available")
                    || err_text.contains("not available")
                {
                    StreamErrorKind::DeviceUnavailable
                } else {
                    StreamErrorKind::Other
                }
            }
        }
    }

    fn process_stream_faults(rx_stream_fault: &flume::Receiver<StreamFault>) -> bool {
        match rx_stream_fault.try_recv() {
            Ok(StreamFault::DeviceUnavailable) => true,
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => false,
        }
    }

    fn set_stream_health(stream_health: &Option<Arc<Mutex<StreamHealth>>>, next: StreamHealth) {
        if let Some(shared) = stream_health {
            let mut guard = shared.lock();
            if *guard != next {
                *guard = next;
            }
        }
    }

    fn report_stream_errors_if_due(
        stream_errors: &Arc<Mutex<StreamErrorTelemetry>>,
        stream_health: &Option<Arc<Mutex<StreamHealth>>>,
    ) {
        let report = {
            let mut guard = stream_errors.lock();
            guard.take_report_if_due(Instant::now())
        };

        let Some((interval, lifetime)) = report else {
            return;
        };
        if interval.total() == 0 {
            Self::set_stream_health(stream_health, StreamHealth::Healthy);
            return;
        }
        Self::set_stream_health(stream_health, StreamHealth::Degraded);

        eprintln!(
            "INFO: audio stream errors (last {}s) xrun={} pollerr={} other={} device_unavailable={} | lifetime xrun={} pollerr={} other={} device_unavailable={}",
            STREAM_ERROR_REPORT_INTERVAL.as_secs(),
            interval.xrun,
            interval.pollerr,
            interval.other,
            interval.device_unavailable,
            lifetime.xrun,
            lifetime.pollerr,
            lifetime.other,
            lifetime.device_unavailable
        );
    }

    fn process_pending_events(
        &mut self,
        rx_events: &flume::Receiver<VisualizerMsg>,
        base_colors: &mut Vec<Oklch>,
        brightness: &mut Vec<f32>,
        prev_max: &mut [f32],
        speed: &mut [f32],
    ) -> bool {
        loop {
            match rx_events.try_recv() {
                Ok(event) => self.update_state(event, base_colors, brightness, prev_max, speed),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => {
                    eprintln!("WARNING: visualizer events channel disconnected; stopping thread.");
                    return false;
                }
            }
        }
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
    pub fn init(mut self) -> flume::Sender<VisualizerMsg> {
        let (tx_events, rx_events) = flume::unbounded();
        thread::spawn(move || {
            panic::register_backtrace_panic_handler();
            Self::set_stream_health(&self.stream_health, StreamHealth::Starting);

            let n = self.nl_udp.panels.len();
            let sample_rate = self.audio_stream.stream_config.sample_rate;
            let mut base_colors = utils::colors_from_rgb(&self.hues, n);
            let mut brightness = vec![0.0_f32; n];
            let mut prev_max = vec![0.0; n];
            let mut speed = vec![0.0; n];
            // Clear any colors left over from a previous Nanoleaf scene or effect
            self.send_black_frame(n);

            let stream_errors = Arc::new(Mutex::new(StreamErrorTelemetry::new()));
            let mut audio_backoff = AUDIO_RESTART_INITIAL_BACKOFF;

            // Outer loop: lifetime of the visualizer thread. The Nanoleaf UDP
            // attachment lives here. Audio cpal streams are created and dropped
            // beneath us so the panels stay attached even if the audio device
            // disappears (e.g. snd-aloop, AirPlay restart).
            'outer: loop {
                if !self.process_pending_events(
                    &rx_events,
                    &mut base_colors,
                    &mut brightness,
                    &mut prev_max,
                    &mut speed,
                ) {
                    break 'outer;
                }
                if matches!(self.state, VisualizerState::Done) {
                    break 'outer;
                }

                let session =
                    Self::build_audio_session(&self.audio_stream, Arc::clone(&stream_errors));
                let (stream, rx_audio, rx_stream_fault) = match session {
                    Some(triple) => triple,
                    None => {
                        Self::set_stream_health(&self.stream_health, StreamHealth::Restarting);
                        if !self.idle_for_audio(
                            audio_backoff,
                            &rx_events,
                            &mut base_colors,
                            &mut brightness,
                            &mut prev_max,
                            &mut speed,
                        ) {
                            break 'outer;
                        }
                        audio_backoff = (audio_backoff * 2).min(AUDIO_RESTART_MAX_BACKOFF);
                        continue 'outer;
                    }
                };
                audio_backoff = AUDIO_RESTART_INITIAL_BACKOFF;
                Self::set_stream_health(&self.stream_health, StreamHealth::Healthy);

                let outcome = self.run_audio_pump(
                    &rx_audio,
                    &rx_stream_fault,
                    &rx_events,
                    &mut base_colors,
                    &mut brightness,
                    &mut prev_max,
                    &mut speed,
                    &stream_errors,
                    sample_rate,
                );
                drop(stream);
                match outcome {
                    AudioLoopExit::Done => break 'outer,
                    AudioLoopExit::RebuildAudio => {
                        Self::set_stream_health(&self.stream_health, StreamHealth::Restarting);
                        eprintln!(
                            "WARNING: audio stream interrupted; rebuilding (Nanoleaf attachment preserved)."
                        );
                    }
                }
            }
            Self::set_stream_health(&self.stream_health, StreamHealth::Stopped);
        });

        tx_events
    }

    /// Construct a fresh cpal capture stream + sample/fault channels.
    /// Returns None on any failure — callers should sleep with backoff and retry.
    fn build_audio_session(
        audio_stream: &AudioStream,
        stream_errors: Arc<Mutex<StreamErrorTelemetry>>,
    ) -> Option<(
        cpal::Stream,
        flume::Receiver<Vec<f32>>,
        flume::Receiver<StreamFault>,
    )> {
        let (tx_audio, rx_audio) = flume::bounded(AUDIO_TX_CHANNEL_DEPTH);
        let (tx_stream_fault, rx_stream_fault) = flume::bounded(8);
        let n_channels = audio_stream.stream_config.channels as usize;

        macro_rules! build_input_stream {
            ($type:ty) => {
                audio_stream.device.build_input_stream(
                    &audio_stream.stream_config,
                    Self::create_data_callback::<$type>(n_channels, tx_audio.clone()),
                    {
                        let tx_stream_fault = tx_stream_fault.clone();
                        let stream_errors = Arc::clone(&stream_errors);
                        move |err| {
                            let err_text = err.to_string();
                            let kind = Self::classify_stream_error(&err, &err_text);
                            let mut telemetry = stream_errors.lock();
                            telemetry.record(kind);
                            if matches!(kind, StreamErrorKind::DeviceUnavailable) {
                                let _ = tx_stream_fault.try_send(StreamFault::DeviceUnavailable);
                            }
                        }
                    },
                    None,
                )
            };
        }

        let stream_result = match audio_stream.sample_format {
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
            _ => {
                eprintln!(
                    "WARNING: Unsupported sample format for live visualizer: {:?}",
                    audio_stream.sample_format
                );
                return None;
            }
        };

        let stream = match stream_result {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("WARNING: stream initialization failed: {}", err);
                return None;
            }
        };
        if let Err(err) = stream.play() {
            eprintln!("WARNING: running the audio stream failed: {}", err);
            return None;
        }
        Some((stream, rx_audio, rx_stream_fault))
    }

    /// Sleep for `duration` while continuing to process control events.
    /// Returns false if the visualizer was asked to stop (End received or the
    /// events channel was dropped) — caller should exit the outer loop.
    #[allow(clippy::too_many_arguments)]
    fn idle_for_audio(
        &mut self,
        duration: Duration,
        rx_events: &flume::Receiver<VisualizerMsg>,
        base_colors: &mut Vec<Oklch>,
        brightness: &mut Vec<f32>,
        prev_max: &mut [f32],
        speed: &mut [f32],
    ) -> bool {
        let deadline = Instant::now() + duration;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            let chunk = (deadline - now).min(Duration::from_millis(100));
            match rx_events.recv_timeout(chunk) {
                Ok(event) => {
                    self.update_state(event, base_colors, brightness, prev_max, speed);
                    if matches!(self.state, VisualizerState::Done) {
                        return false;
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    /// Inner audio pump. Drives panels from FFT output until either End is
    /// received (Done) or the audio stream goes away (RebuildAudio).
    #[allow(clippy::too_many_arguments)]
    fn run_audio_pump(
        &mut self,
        rx_audio: &flume::Receiver<Vec<f32>>,
        rx_stream_fault: &flume::Receiver<StreamFault>,
        rx_events: &flume::Receiver<VisualizerMsg>,
        base_colors: &mut Vec<Oklch>,
        brightness: &mut Vec<f32>,
        prev_max: &mut [f32],
        speed: &mut [f32],
        stream_errors: &Arc<Mutex<StreamErrorTelemetry>>,
        sample_rate: u32,
    ) -> AudioLoopExit {
        loop {
            Self::report_stream_errors_if_due(stream_errors, &self.stream_health);
            if matches!(self.state, VisualizerState::Done) {
                return AudioLoopExit::Done;
            }
            if Self::process_stream_faults(rx_stream_fault) {
                return AudioLoopExit::RebuildAudio;
            }
            if !self.process_pending_events(rx_events, base_colors, brightness, prev_max, speed) {
                return AudioLoopExit::Done;
            }

            let to_collect = ((sample_rate as f32) * self.time_window).round() as usize;
            let max_buffered = to_collect.saturating_mul(4);
            let mut samples = Vec::with_capacity(2 * to_collect);
            while samples.len() < to_collect {
                Self::report_stream_errors_if_due(stream_errors, &self.stream_health);
                if Self::process_stream_faults(rx_stream_fault) {
                    return AudioLoopExit::RebuildAudio;
                }
                let mut new_samples = match rx_audio.recv_timeout(AUDIO_RECV_TIMEOUT) {
                    Ok(samples) => samples,
                    Err(RecvTimeoutError::Timeout) => {
                        if Self::process_stream_faults(rx_stream_fault) {
                            return AudioLoopExit::RebuildAudio;
                        }
                        if !self.process_pending_events(
                            rx_events,
                            base_colors,
                            brightness,
                            prev_max,
                            speed,
                        ) {
                            return AudioLoopExit::Done;
                        }
                        if matches!(self.state, VisualizerState::Done) {
                            return AudioLoopExit::Done;
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        // The cpal callback's tx_audio was dropped — the stream
                        // is gone. Rebuild rather than tearing down the thread.
                        return AudioLoopExit::RebuildAudio;
                    }
                };
                samples.append(&mut new_samples);
                // If we've buffered far more than this frame needs (consumer
                // is behind a producer burst), drop the oldest excess so we
                // catch up to real time instead of accumulating latency.
                if samples.len() > max_buffered {
                    let drop_count = samples.len() - to_collect.saturating_mul(2);
                    samples.drain(..drop_count);
                }
            }

            let spectrum = processing::process(samples, self.gain);
            let hz_per_bin = (sample_rate / 2) / (spectrum.len() as u32);
            match self.effect {
                Effect::Spectrum => processing::update_brightness(
                    spectrum,
                    hz_per_bin,
                    self.min_freq,
                    self.max_freq,
                    brightness,
                    prev_max,
                    speed,
                ),
                Effect::EnergyWave => processing::update_brightness_wave(
                    spectrum,
                    hz_per_bin,
                    self.min_freq,
                    self.max_freq,
                    brightness,
                    prev_max,
                    speed,
                ),
                Effect::Ripple => processing::update_brightness_ripple(
                    spectrum,
                    hz_per_bin,
                    self.min_freq,
                    self.max_freq,
                    brightness,
                    prev_max,
                    speed,
                ),
            }
            let display_colors: Vec<Oklch> = base_colors
                .iter()
                .zip(brightness.iter())
                .map(|(base, &b)| Oklch::new(base.l * b, base.chroma, base.hue))
                .collect();
            if self
                .nl_udp
                .update_panels(&display_colors, self.trans_time)
                .is_err()
                && self.nl_device.request_udp_control().is_ok()
            {
                let _ = self.nl_udp.update_panels(&display_colors, self.trans_time);
            }
            if !self.color_subscribers.is_empty() {
                let mut frame = HashMap::with_capacity(display_colors.len());
                for (i, color) in display_colors.iter().enumerate() {
                    let srgb: Srgb<f32> = Srgb::from_color(*color);
                    let r = (srgb.red.clamp(0.0, 1.0) * 255.0) as u8;
                    let g = (srgb.green.clamp(0.0, 1.0) * 255.0) as u8;
                    let b = (srgb.blue.clamp(0.0, 1.0) * 255.0) as u8;
                    frame.insert(self.nl_udp.panels[i].id, [r, g, b]);
                }
                for tx in &self.color_subscribers {
                    let _ = tx.try_send(frame.clone());
                }
            }
        }
    }
}
