use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
#[cfg(target_os = "linux")]
use base64::Engine;
use clap::Parser;
use hashbrown::HashMap;
use parking_lot::Mutex;
#[cfg(target_os = "linux")]
use quick_xml::Reader as XmlReader;
#[cfg(target_os = "linux")]
use quick_xml::events::Event as XmlEvent;
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::thread;
#[cfg(target_os = "linux")]
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader},
};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::task::JoinError;
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
};

#[derive(Parser, Debug)]
#[command(version, about = "Audioleaf — Nanoleaf music visualizer", author)]
struct ApiOptions {
    /// Host interface to bind
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// HTTP port for the API server
    #[arg(long, default_value_t = 8787)]
    port: u16,

    /// Path to audioleaf's configuration file
    #[arg(long = "config")]
    config_file_path: Option<PathBuf>,

    /// Path to audioleaf's database of known Nanoleaf devices
    #[arg(long = "devices")]
    devices_file_path: Option<PathBuf>,
}

#[derive(Clone)]
struct ApiState {
    config_file_path: Option<PathBuf>,
    devices_file_path: Option<PathBuf>,
    runtime_config: Arc<Mutex<audioleaf::config::Config>>,
    live_visualizer: Arc<Mutex<Option<LiveVisualizerRuntime>>>,
    live_visualizer_recovery: Arc<Mutex<LiveVisualizerRecoveryState>>,
    now_playing: Arc<Mutex<NowPlayingRuntimeState>>,
    /// 5-min in-memory cache of the active Nanoleaf device's saved palettes.
    /// Replaces the static `src/palettes.rs` catalog. None until the first
    /// successful fetch; refreshed lazily on read once stale.
    palette_cache: Arc<Mutex<PaletteCacheSlot>>,
}

type PaletteCacheSlot = Option<(Instant, Vec<audioleaf::nanoleaf::NamedPalette>)>;

const PALETTE_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
struct LiveVisualizerRuntime {
    sender: flume::Sender<audioleaf::visualizer::VisualizerMsg>,
    global_orientation: u16,
    device: DeviceSummary,
    color_rx: flume::Receiver<HashMap<u16, [u8; 3]>>,
    latest_colors: Arc<Mutex<HashMap<u16, [u8; 3]>>>,
    stream_health: Arc<Mutex<audioleaf::visualizer::StreamHealth>>,
}

#[derive(Clone, Debug, Default)]
struct LiveVisualizerRecoveryState {
    consecutive_restart_failures: u32,
    auto_fallback_to_default_active: bool,
    last_restart_at_ms: Option<u64>,
    healthy_ping_streak: u8,
}

#[cfg(target_os = "linux")]
const DEFAULT_SHAIRPORT_METADATA_PIPE: &str = "/tmp/shairport-sync-metadata";
#[cfg(target_os = "linux")]
const NOW_PLAYING_RETRY_DELAY: Duration = Duration::from_secs(3);
const LIVE_VISUALIZER_WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);
const LIVE_VISUALIZER_RESTART_FAILURE_LIMIT: u32 = 3;
const LIVE_VISUALIZER_RESTART_COOLDOWN: Duration = Duration::from_secs(1);
const LIVE_VISUALIZER_RESTART_COOLDOWN_MAX: Duration = Duration::from_secs(60);
const LIVE_VISUALIZER_HEALTHY_PINGS_TO_CLEAR_FAILURES: u8 = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Debug, Default)]
struct NowPlayingTrackData {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    genre: Option<String>,
    composer: Option<String>,
    stream_url: Option<String>,
    source_name: Option<String>,
    source_ip: Option<String>,
    user_agent: Option<String>,
    /// Song data kind: 0 = timed track (has duration), 1 = untimed stream (radio)
    song_data_kind: Option<u32>,
    /// Track duration in milliseconds (from DMAP "astm" code)
    duration_ms: Option<u64>,
}

impl NowPlayingTrackData {
    fn has_data(&self) -> bool {
        self.title.as_deref().is_some_and(|value| !value.is_empty())
            || self
                .artist
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || self.album.as_deref().is_some_and(|value| !value.is_empty())
            || self
                .stream_url
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || self
                .source_name
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || self
                .source_ip
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || self
                .user_agent
                .as_deref()
                .is_some_and(|value| !value.is_empty())
    }
}

#[derive(Clone, Debug)]
struct NowPlayingRuntimeState {
    metadata_pipe_path: String,
    reader_running: bool,
    last_error: Option<String>,
    track: NowPlayingTrackData,
    /// Colors extracted from album artwork. Drives the visualizer when
    /// `visualizer_config.color_source == Artwork`. Empty between tracks /
    /// when no album cover is available.
    palette_colors: Vec<[u8; 3]>,
    artwork_bytes: Option<Vec<u8>>,
    artwork_mime_type: Option<String>,
    artwork_generation: u64,
    updated_at_ms: Option<u64>,
    playback_state: PlaybackState,
    /// Progress from "prgr": RTP timestamps at 44100 fps as (start, current, end)
    progress_rtp: Option<(u64, u64, u64)>,
    /// Wall-clock unix-ms timestamp when `progress_rtp` was last updated.
    /// Used to extrapolate elapsed time between sparse `prgr` events
    /// (shairport only emits one at stream start + on flushes).
    progress_received_at_ms: Option<u64>,
    /// AirPlay volume in dB (0.0 to -30.0, -144.0 = mute)
    volume_db: Option<f32>,
}

impl NowPlayingRuntimeState {
    fn new(metadata_pipe_path: String) -> Self {
        Self {
            metadata_pipe_path,
            reader_running: false,
            last_error: None,
            track: NowPlayingTrackData::default(),
            palette_colors: Vec::new(),
            artwork_bytes: None,
            artwork_mime_type: None,
            artwork_generation: 0,
            updated_at_ms: None,
            playback_state: PlaybackState::default(),
            progress_rtp: None,
            progress_received_at_ms: None,
            volume_db: None,
        }
    }

    #[cfg(target_os = "linux")]
    fn clear_session_data(&mut self) {
        self.track = NowPlayingTrackData::default();
        self.palette_colors.clear();
        self.artwork_bytes = None;
        self.artwork_mime_type = None;
        self.artwork_generation = self.artwork_generation.saturating_add(1);
        self.progress_rtp = None;
        self.progress_received_at_ms = None;
        self.updated_at_ms = Some(now_unix_ms());
    }

    /// Returns (elapsed_secs, total_secs) derived from RTP timestamps at 44100 Hz.
    /// While `playback_state == Playing`, the elapsed value is extrapolated
    /// forward from `progress_received_at_ms` since shairport only emits
    /// `prgr` at stream start / flushes — the raw `current` would otherwise
    /// stay frozen at whatever shairport last reported.
    fn progress_seconds(&self) -> Option<(f64, f64)> {
        let (start, current, end) = self.progress_rtp?;
        let raw_elapsed = current.wrapping_sub(start) as f64 / 44100.0;
        let total = end.wrapping_sub(start) as f64 / 44100.0;
        let elapsed = match (self.progress_received_at_ms, &self.playback_state) {
            (Some(anchor_ms), PlaybackState::Playing) => {
                let now_ms = now_unix_ms();
                let drift = now_ms.saturating_sub(anchor_ms) as f64 / 1000.0;
                (raw_elapsed + drift).min(total)
            }
            _ => raw_elapsed,
        };
        Some((elapsed, total))
    }

    fn snapshot(&self) -> NowPlayingResponse {
        let (progress_elapsed_secs, progress_total_secs) = self
            .progress_seconds()
            .map(|(e, t)| (Some(e), Some(t)))
            .unwrap_or((None, None));
        NowPlayingResponse {
            reader_running: self.reader_running,
            metadata_pipe_path: self.metadata_pipe_path.clone(),
            last_error: self.last_error.clone(),
            track: self.track.has_data().then_some(NowPlayingTrackResponse {
                title: self.track.title.clone(),
                artist: self.track.artist.clone(),
                album: self.track.album.clone(),
                genre: self.track.genre.clone(),
                composer: self.track.composer.clone(),
                stream_url: self.track.stream_url.clone(),
                source_name: self.track.source_name.clone(),
                source_ip: self.track.source_ip.clone(),
                user_agent: self.track.user_agent.clone(),
                duration_ms: self.track.duration_ms,
                song_data_kind: self.track.song_data_kind,
            }),
            palette_colors: self.palette_colors.clone(),
            artwork_available: self.artwork_bytes.is_some(),
            artwork_generation: self.artwork_generation,
            updated_at_ms: self.updated_at_ms,
            playback_state: self.playback_state.clone(),
            progress_elapsed_secs,
            progress_total_secs,
            volume_db: self.volume_db,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct NowPlayingTrackResponse {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    genre: Option<String>,
    composer: Option<String>,
    stream_url: Option<String>,
    source_name: Option<String>,
    source_ip: Option<String>,
    user_agent: Option<String>,
    duration_ms: Option<u64>,
    song_data_kind: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct NowPlayingResponse {
    reader_running: bool,
    metadata_pipe_path: String,
    last_error: Option<String>,
    track: Option<NowPlayingTrackResponse>,
    palette_colors: Vec<[u8; 3]>,
    artwork_available: bool,
    artwork_generation: u64,
    updated_at_ms: Option<u64>,
    playback_state: PlaybackState,
    progress_elapsed_secs: Option<f64>,
    progress_total_secs: Option<f64>,
    volume_db: Option<f32>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn internal<E: std::fmt::Display>(err: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }

    fn not_found<E: std::fmt::Display>(err: E) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: err.to_string(),
        }
    }

    fn bad_request<E: std::fmt::Display>(err: E) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: err.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct PathsResponse {
    config_file_path: String,
    config_file_exists: bool,
    devices_file_path: String,
    devices_file_exists: bool,
}

#[derive(Debug, Serialize)]
struct ConfigResponse {
    paths: PathsResponse,
    config: Option<audioleaf::config::Config>,
}

#[derive(Clone, Debug, Serialize)]
struct DeviceSummary {
    name: String,
    ip: String,
}

#[derive(Debug, Serialize)]
struct DevicesResponse {
    devices: Vec<DeviceSummary>,
    devices_file_path: String,
    devices_file_exists: bool,
}

#[derive(Debug, Serialize)]
struct DeviceInfoResponse {
    device: DeviceSummary,
    info: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct DeviceLayoutPanel {
    panel_id: u16,
    x: i16,
    y: i16,
    orientation: u16,
    shape_type_id: u64,
    shape_type_name: String,
    num_sides: usize,
    side_length: f32,
}

#[derive(Debug, Serialize)]
struct DeviceLayoutResponse {
    device: DeviceSummary,
    global_orientation: u16,
    panels: Vec<DeviceLayoutPanel>,
}

#[derive(Debug, Deserialize)]
struct VisualizerEffectUpdateRequest {
    effect: String,
}

#[derive(Debug, Deserialize)]
struct VisualizerPaletteUpdateRequest {
    palette_name: String,
}

#[derive(Debug, Deserialize)]
struct VisualizerColorSourceUpdateRequest {
    kind: String,
    /// Optional. Only honored when `kind == "palette"`. Leaving it unset (or
    /// passing the same name) leaves the existing `palette_name` config alone.
    palette_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisualizerSortUpdateRequest {
    primary_axis: String,
    sort_primary: String,
    sort_secondary: String,
}

#[derive(Debug, Deserialize)]
struct VisualizerSettingsUpdateRequest {
    audio_backend: Option<String>,
    freq_range: Option<(u16, u16)>,
    default_gain: Option<f32>,
    transition_time: Option<u16>,
    time_window: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct DeviceStateUpdateRequest {
    power_on: Option<bool>,
    brightness: Option<u8>,
}

#[derive(Debug, Serialize)]
struct DeviceStateUpdateResponse {
    device: DeviceSummary,
    power_on: Option<bool>,
    brightness: Option<u8>,
}

#[derive(Debug, Serialize)]
struct PaletteEntry {
    name: String,
    colors: Vec<[u8; 3]>,
}

#[derive(Debug, Serialize)]
struct PalettesResponse {
    palettes: Vec<PaletteEntry>,
}

#[derive(Debug, Serialize)]
struct AudioBackendsResponse {
    current_audio_backend: Option<String>,
    available_audio_backends: Vec<String>,
}

#[derive(Debug, Serialize)]
struct VisualizerPreviewPanelColor {
    panel_id: u16,
    rgb: [u8; 3],
}

#[derive(Debug, Serialize)]
struct VisualizerPreviewResponse {
    enabled: bool,
    device: Option<DeviceSummary>,
    panel_colors: Vec<VisualizerPreviewPanelColor>,
}

#[derive(Debug, Serialize)]
struct VisualizerStatusResponse {
    status: String,
    stream_health: String,
    live_visualizer_attached: bool,
    restart_cooldown_active: bool,
    consecutive_restart_failures: u32,
    healthy_ping_streak: u8,
    auto_fallback_to_default_active: bool,
    current_audio_backend: Option<String>,
    device: Option<DeviceSummary>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = ApiOptions::parse();
    let ((resolved_config_path, config_exists), _) = audioleaf::config::resolve_paths(
        options.config_file_path.clone(),
        options.devices_file_path.clone(),
    )?;
    let initial_config = if config_exists {
        audioleaf::config::Config::parse_from_file(&resolved_config_path)?
    } else {
        audioleaf::config::Config::new(None, None)
    };
    #[cfg(target_os = "linux")]
    let metadata_pipe_path = std::env::var("AUDIOLEAF_SHAIRPORT_METADATA_PIPE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_SHAIRPORT_METADATA_PIPE.to_string());
    #[cfg(not(target_os = "linux"))]
    let metadata_pipe_path = String::new();

    let state = ApiState {
        config_file_path: options.config_file_path,
        devices_file_path: options.devices_file_path,
        runtime_config: Arc::new(Mutex::new(initial_config)),
        live_visualizer: Arc::new(Mutex::new(None)),
        live_visualizer_recovery: Arc::new(Mutex::new(LiveVisualizerRecoveryState::default())),
        now_playing: Arc::new(Mutex::new(NowPlayingRuntimeState::new(
            metadata_pipe_path.clone(),
        ))),
        palette_cache: Arc::new(Mutex::new(None)),
    };

    if let Err(err) = restart_live_visualizer(&state).await {
        eprintln!(
            "WARNING: Live visualizer startup failed. API will still run, but effect changes will not be applied live: {}",
            err.message
        );
    } else {
        println!("Live visualizer initialized.");
    }
    start_now_playing_reader(&state);
    start_live_visualizer_watchdog(&state);

    let app = Router::new()
        .route("/api/health", get(get_health))
        .route("/api/config", get(get_config))
        .route("/api/config/save", post(post_config_save))
        .route("/api/config/visualizer/effect", put(put_visualizer_effect))
        .route(
            "/api/config/visualizer/palette",
            put(put_visualizer_palette),
        )
        .route(
            "/api/config/visualizer/color-source",
            put(put_visualizer_color_source),
        )
        .route("/api/config/visualizer/sort", put(put_visualizer_sort))
        .route(
            "/api/config/visualizer/settings",
            put(put_visualizer_settings),
        )
        .route("/api/now-playing", get(get_now_playing))
        .route("/api/now-playing/artwork", get(get_now_playing_artwork))
        .route("/api/visualizer/preview", get(get_visualizer_preview))
        .route("/api/visualizer/ws", get(visualizer_ws))
        .route("/api/visualizer/status", get(get_visualizer_status))
        .route("/api/audio/backends", get(get_audio_backends))
        .route("/api/devices", get(get_devices))
        .route("/api/devices/{name}/info", get(get_device_info))
        .route("/api/devices/{name}/layout", get(get_device_layout))
        .route("/api/devices/{name}/state", put(put_device_state))
        .route("/api/palettes", get(get_palettes))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // Serve the built frontend from web/dist/ (or AUDIOLEAF_FRONTEND_DIR override).
    // Falls back to index.html for client-side SPA routing.
    let frontend_dir =
        std::env::var("AUDIOLEAF_FRONTEND_DIR").unwrap_or_else(|_| "./web/dist".to_string());
    let frontend_path = std::path::Path::new(&frontend_dir);
    let app = if frontend_path.is_dir() {
        let index = format!("{}/index.html", frontend_dir);
        app.fallback_service(ServeDir::new(&frontend_dir).fallback(ServeFile::new(index)))
    } else {
        app
    };

    let addr: SocketAddr = format!("{}:{}", options.host, options.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!(
        "Audioleaf API listening on http://{}",
        listener.local_addr()?
    );
    if frontend_path.is_dir() {
        println!("Serving frontend from {}", frontend_dir);
    }
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

async fn get_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn get_config(State(state): State<ApiState>) -> ApiResult<ConfigResponse> {
    let paths = resolve_paths(&state)?;
    let config = Some(get_runtime_config_clone(&state)?);

    Ok(Json(ConfigResponse { paths, config }))
}

async fn post_config_save(State(state): State<ApiState>) -> ApiResult<ConfigResponse> {
    let config = get_runtime_config_clone(&state)?;
    let mut paths = resolve_paths(&state)?;
    config
        .write_to_file(PathBuf::from(&paths.config_file_path).as_path())
        .map_err(ApiError::internal)?;
    paths.config_file_exists = true;

    Ok(Json(ConfigResponse {
        paths,
        config: Some(config),
    }))
}

async fn put_visualizer_effect(
    State(state): State<ApiState>,
    Json(payload): Json<VisualizerEffectUpdateRequest>,
) -> ApiResult<ConfigResponse> {
    let effect = parse_effect(&payload.effect).ok_or_else(|| {
        ApiError::bad_request("Invalid effect. Use Spectrum, EnergyWave, or Ripple.")
    })?;

    let config = update_runtime_config(&state, |config| {
        config.visualizer_config.effect = Some(effect);
    })?;
    let paths = resolve_paths(&state)?;
    send_live_message_with_recovery(
        &state,
        audioleaf::visualizer::VisualizerMsg::SetEffect(effect),
    )
    .await?;

    Ok(Json(ConfigResponse {
        paths,
        config: Some(config),
    }))
}

async fn put_visualizer_palette(
    State(state): State<ApiState>,
    Json(payload): Json<VisualizerPaletteUpdateRequest>,
) -> ApiResult<ConfigResponse> {
    let palettes = palettes_cached_async(&state).await;
    if !palettes.iter().any(|p| p.name == payload.palette_name) {
        let mut names: Vec<String> = palettes.into_iter().map(|p| p.name).collect();
        names.sort();
        return Err(ApiError::bad_request(format!(
            "Unknown palette '{}'. Available on device: {}",
            payload.palette_name,
            if names.is_empty() {
                "(no palettes — Nanoleaf device may be offline)".to_string()
            } else {
                names.join(", ")
            }
        )));
    }

    let config = update_runtime_config(&state, |config| {
        config.visualizer_config.color_source = Some(audioleaf::config::ColorSourceKind::Palette);
        config.visualizer_config.palette_name = Some(payload.palette_name.clone());
    })?;

    let colors = resolve_palette_colors_async(&state, Some(payload.palette_name.clone())).await;
    let paths = resolve_paths(&state)?;
    // Picking a palette implies leaving artwork-idle if we were in it.
    send_live_message_with_recovery(
        &state,
        audioleaf::visualizer::VisualizerMsg::SetIdleColor(None),
    )
    .await?;
    send_live_message_with_recovery(
        &state,
        audioleaf::visualizer::VisualizerMsg::SetPalette(colors),
    )
    .await?;

    Ok(Json(ConfigResponse {
        paths,
        config: Some(config),
    }))
}

async fn put_visualizer_color_source(
    State(state): State<ApiState>,
    Json(payload): Json<VisualizerColorSourceUpdateRequest>,
) -> ApiResult<ConfigResponse> {
    let kind = match payload.kind.to_ascii_lowercase().as_str() {
        "palette" => audioleaf::config::ColorSourceKind::Palette,
        "artwork" => audioleaf::config::ColorSourceKind::Artwork,
        other => {
            return Err(ApiError::bad_request(format!(
                "color_source kind must be `palette` or `artwork`, got `{}`",
                other
            )));
        }
    };

    let config = update_runtime_config(&state, |config| {
        config.visualizer_config.color_source = Some(kind);
        if kind == audioleaf::config::ColorSourceKind::Palette && payload.palette_name.is_some() {
            config.visualizer_config.palette_name = payload.palette_name.clone();
        }
    })?;

    let paths = resolve_paths(&state)?;

    match kind {
        audioleaf::config::ColorSourceKind::Palette => {
            let colors =
                resolve_palette_colors_async(&state, config.visualizer_config.palette_name.clone())
                    .await;
            send_live_message_with_recovery(
                &state,
                audioleaf::visualizer::VisualizerMsg::SetIdleColor(None),
            )
            .await?;
            send_live_message_with_recovery(
                &state,
                audioleaf::visualizer::VisualizerMsg::SetPalette(colors),
            )
            .await?;
        }
        audioleaf::config::ColorSourceKind::Artwork => {
            // Idle controller decides idle vs artwork-driven from current
            // playback / artwork state.
            update_idle_state_async(&state).await;
        }
    }

    Ok(Json(ConfigResponse {
        paths,
        config: Some(config),
    }))
}

async fn put_visualizer_sort(
    State(state): State<ApiState>,
    Json(payload): Json<VisualizerSortUpdateRequest>,
) -> ApiResult<ConfigResponse> {
    let primary_axis = parse_axis(&payload.primary_axis)
        .ok_or_else(|| ApiError::bad_request("Invalid primary_axis. Use X or Y."))?;
    let sort_primary = parse_sort(&payload.sort_primary)
        .ok_or_else(|| ApiError::bad_request("Invalid sort_primary. Use Asc or Desc."))?;
    let sort_secondary = parse_sort(&payload.sort_secondary)
        .ok_or_else(|| ApiError::bad_request("Invalid sort_secondary. Use Asc or Desc."))?;

    let config = update_runtime_config(&state, |config| {
        config.visualizer_config.primary_axis = Some(primary_axis);
        config.visualizer_config.sort_primary = Some(sort_primary);
        config.visualizer_config.sort_secondary = Some(sort_secondary);
    })?;
    let paths = resolve_paths(&state)?;
    let live = ensure_live_visualizer(&state).await?;
    send_live_message_with_recovery(
        &state,
        audioleaf::visualizer::VisualizerMsg::SetSorting {
            primary_axis,
            sort_primary,
            sort_secondary,
            global_orientation: live.global_orientation,
        },
    )
    .await?;

    Ok(Json(ConfigResponse {
        paths,
        config: Some(config),
    }))
}

async fn put_visualizer_settings(
    State(state): State<ApiState>,
    Json(payload): Json<VisualizerSettingsUpdateRequest>,
) -> ApiResult<ConfigResponse> {
    if payload.audio_backend.is_none()
        && payload.freq_range.is_none()
        && payload.default_gain.is_none()
        && payload.transition_time.is_none()
        && payload.time_window.is_none()
    {
        return Err(ApiError::bad_request(
            "Request must include at least one visualizer setting.",
        ));
    }

    if let Some((low, high)) = payload.freq_range
        && low >= high
    {
        return Err(ApiError::bad_request(
            "freq_range must have min < max (e.g. [20, 4500]).",
        ));
    }
    if let Some(default_gain) = payload.default_gain
        && (!default_gain.is_finite() || default_gain < 0.0)
    {
        return Err(ApiError::bad_request(
            "default_gain must be a finite number >= 0.",
        ));
    }
    if let Some(transition_time) = payload.transition_time
        && !(1..=10).contains(&transition_time)
    {
        return Err(ApiError::bad_request(
            "transition_time must be between 1 and 10 (0.1s to 1.0s in 100ms units).",
        ));
    }
    if let Some(time_window) = payload.time_window
        && (!time_window.is_finite() || !(0.1..=1.0).contains(&time_window))
    {
        return Err(ApiError::bad_request(
            "time_window must be between 0.1 and 1.0 seconds.",
        ));
    }

    let audio_backend = payload.audio_backend.clone();
    let freq_range = payload.freq_range;
    let default_gain = payload.default_gain;
    let transition_time = payload.transition_time;
    let time_window = payload.time_window;

    let config = update_runtime_config(&state, |config| {
        if let Some(audio_backend) = audio_backend.clone() {
            config.visualizer_config.audio_backend = Some(audio_backend);
        }
        if let Some(freq_range) = freq_range {
            config.visualizer_config.freq_range = Some(freq_range);
        }
        if let Some(default_gain) = default_gain {
            config.visualizer_config.default_gain = Some(default_gain);
        }
        if let Some(transition_time) = transition_time {
            config.visualizer_config.transition_time = Some(transition_time);
        }
        if let Some(time_window) = time_window {
            config.visualizer_config.time_window = Some(time_window);
        }
    })?;
    let paths = resolve_paths(&state)?;

    if payload.audio_backend.is_some() {
        restart_live_visualizer(&state).await?;
    } else {
        if let Some(freq_range) = payload.freq_range {
            send_live_message_with_recovery(
                &state,
                audioleaf::visualizer::VisualizerMsg::SetFreqRange(freq_range.0, freq_range.1),
            )
            .await?;
        }
        if let Some(default_gain) = payload.default_gain {
            send_live_message_with_recovery(
                &state,
                audioleaf::visualizer::VisualizerMsg::SetGain(default_gain),
            )
            .await?;
        }
        if let Some(transition_time) = payload.transition_time {
            send_live_message_with_recovery(
                &state,
                audioleaf::visualizer::VisualizerMsg::SetTransitionTime(transition_time),
            )
            .await?;
        }
        if let Some(time_window) = payload.time_window {
            send_live_message_with_recovery(
                &state,
                audioleaf::visualizer::VisualizerMsg::SetTimeWindow(time_window),
            )
            .await?;
        }
    }

    Ok(Json(ConfigResponse {
        paths,
        config: Some(config),
    }))
}

async fn get_now_playing(State(state): State<ApiState>) -> ApiResult<NowPlayingResponse> {
    let snapshot = current_now_playing_snapshot(&state)?;
    Ok(Json(snapshot))
}

async fn get_now_playing_artwork(
    State(state): State<ApiState>,
) -> std::result::Result<Response, ApiError> {
    let (bytes, mime_type) = {
        let guard = state.now_playing.lock();
        let Some(bytes) = guard.artwork_bytes.clone() else {
            return Err(ApiError::not_found("No album artwork available yet."));
        };
        let mime_type = guard
            .artwork_mime_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());
        (bytes, mime_type)
    };

    let mut response = Response::new(Body::from(bytes));
    let content_type = header::HeaderValue::from_str(&mime_type)
        .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

async fn get_devices(State(state): State<ApiState>) -> ApiResult<DevicesResponse> {
    let paths = resolve_paths(&state)?;

    let devices = if paths.devices_file_exists {
        audioleaf::nanoleaf::NlDevice::all_from_file(
            PathBuf::from(&paths.devices_file_path).as_path(),
        )
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|device| DeviceSummary {
            name: device.name,
            ip: device.ip.to_string(),
        })
        .collect()
    } else {
        Vec::new()
    };

    Ok(Json(DevicesResponse {
        devices,
        devices_file_path: paths.devices_file_path,
        devices_file_exists: paths.devices_file_exists,
    }))
}

async fn get_device_info(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> ApiResult<DeviceInfoResponse> {
    let device = load_device_by_name(&state, &name)?;

    let summary = DeviceSummary {
        name: device.name.clone(),
        ip: device.ip.to_string(),
    };

    let info = run_nanoleaf_io(move || device.get_device_info()).await?;

    Ok(Json(DeviceInfoResponse {
        device: summary,
        info,
    }))
}

async fn get_device_layout(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> ApiResult<DeviceLayoutResponse> {
    let device = load_device_by_name(&state, &name)?;
    let layout_device = device.clone();

    let (layout_json, orientation_json) = run_nanoleaf_io(move || {
        let layout = layout_device.get_panel_layout()?;
        let orientation = layout_device.get_global_orientation()?;
        Ok((layout, orientation))
    })
    .await?;

    let panels = audioleaf::layout_visualizer::parse_layout(&layout_json)
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|panel| DeviceLayoutPanel {
            panel_id: panel.panel_id,
            x: panel.x,
            y: panel.y,
            orientation: panel.orientation,
            shape_type_id: panel.shape_type.id,
            shape_type_name: panel.shape_type.name.to_string(),
            num_sides: panel.shape_type.num_sides(),
            side_length: panel.shape_type.side_length,
        })
        .collect();

    let global_orientation = orientation_json["value"].as_u64().unwrap_or(0) as u16;

    Ok(Json(DeviceLayoutResponse {
        device: DeviceSummary {
            name: device.name,
            ip: device.ip.to_string(),
        },
        global_orientation,
        panels,
    }))
}

async fn put_device_state(
    Path(name): Path<String>,
    State(state): State<ApiState>,
    Json(payload): Json<DeviceStateUpdateRequest>,
) -> ApiResult<DeviceStateUpdateResponse> {
    if payload.power_on.is_none() && payload.brightness.is_none() {
        return Err(ApiError::bad_request(
            "Request must include `power_on` and/or `brightness`.",
        ));
    }
    if payload
        .brightness
        .is_some_and(|brightness| brightness > 100)
    {
        return Err(ApiError::bad_request(
            "`brightness` must be between 0 and 100.",
        ));
    }

    let device = load_device_by_name(&state, &name)?;
    let write_device = device.clone();
    run_nanoleaf_io(move || write_device.set_state(payload.power_on, payload.brightness)).await?;

    Ok(Json(DeviceStateUpdateResponse {
        device: DeviceSummary {
            name: device.name,
            ip: device.ip.to_string(),
        },
        power_on: payload.power_on,
        brightness: payload.brightness,
    }))
}

async fn get_palettes(State(state): State<ApiState>) -> Json<PalettesResponse> {
    let mut palettes: Vec<PaletteEntry> = palettes_cached_async(&state)
        .await
        .into_iter()
        .map(|p| PaletteEntry {
            name: p.name,
            colors: p.colors,
        })
        .collect();
    palettes.sort_by(|a, b| a.name.cmp(&b.name));
    Json(PalettesResponse { palettes })
}

async fn get_audio_backends(State(state): State<ApiState>) -> ApiResult<AudioBackendsResponse> {
    let current_audio_backend = get_runtime_config_clone(&state)?
        .visualizer_config
        .audio_backend;

    let mut available_audio_backends =
        audioleaf::audio::list_input_backend_names().unwrap_or_else(|_| Vec::new());

    if !available_audio_backends
        .iter()
        .any(|name| name == audioleaf::constants::DEFAULT_AUDIO_BACKEND)
    {
        available_audio_backends.insert(0, audioleaf::constants::DEFAULT_AUDIO_BACKEND.to_string());
    }

    Ok(Json(AudioBackendsResponse {
        current_audio_backend,
        available_audio_backends,
    }))
}

fn latest_panel_colors(runtime: &LiveVisualizerRuntime) -> HashMap<u16, [u8; 3]> {
    let mut latest = None;
    while let Ok(frame) = runtime.color_rx.try_recv() {
        latest = Some(frame);
    }
    if let Some(frame) = latest {
        *runtime.latest_colors.lock() = frame;
    }
    runtime.latest_colors.lock().clone()
}

async fn get_visualizer_preview(
    State(state): State<ApiState>,
) -> ApiResult<VisualizerPreviewResponse> {
    let Some(runtime) = current_live_visualizer(&state)? else {
        return Ok(Json(VisualizerPreviewResponse {
            enabled: false,
            device: None,
            panel_colors: Vec::new(),
        }));
    };

    let colors_map = latest_panel_colors(&runtime);
    let mut panel_colors: Vec<VisualizerPreviewPanelColor> = colors_map
        .iter()
        .map(|(panel_id, rgb)| VisualizerPreviewPanelColor {
            panel_id: *panel_id,
            rgb: *rgb,
        })
        .collect();
    panel_colors.sort_by_key(|entry| entry.panel_id);

    Ok(Json(VisualizerPreviewResponse {
        enabled: true,
        device: Some(runtime.device),
        panel_colors,
    }))
}

async fn visualizer_ws(ws: WebSocketUpgrade, State(state): State<ApiState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_visualizer_ws(socket, state))
}

async fn handle_visualizer_ws(mut socket: WebSocket, state: ApiState) {
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let mut last_frame: HashMap<u16, [u8; 3]> = HashMap::new();
    loop {
        interval.tick().await;
        let Ok(Some(runtime)) = current_live_visualizer(&state) else {
            continue;
        };
        let frame = latest_panel_colors(&runtime);
        if frame == last_frame {
            continue;
        }
        last_frame = frame.clone();
        let mut colors: Vec<VisualizerPreviewPanelColor> = frame
            .iter()
            .map(|(id, rgb)| VisualizerPreviewPanelColor {
                panel_id: *id,
                rgb: *rgb,
            })
            .collect();
        colors.sort_by_key(|c| c.panel_id);
        let Ok(msg) = serde_json::to_string(&colors) else {
            continue;
        };
        if socket.send(Message::Text(msg.into())).await.is_err() {
            break;
        }
    }
}

async fn get_visualizer_status(
    State(state): State<ApiState>,
) -> ApiResult<VisualizerStatusResponse> {
    let live = current_live_visualizer(&state)?;
    let live_visualizer_attached = live.is_some();
    let device = live.as_ref().map(|runtime| runtime.device.clone());
    let stream_health = match live {
        Some(runtime) => *runtime.stream_health.lock(),
        None => audioleaf::visualizer::StreamHealth::Restarting,
    };

    let restart_cooldown_active = live_visualizer_restart_cooldown_remaining(&state)?.is_some();
    let recovery = state.live_visualizer_recovery.lock();
    let current_audio_backend = get_runtime_config_clone(&state)?
        .visualizer_config
        .audio_backend;

    let status = summarize_visualizer_status(
        live_visualizer_attached,
        stream_health,
        recovery.consecutive_restart_failures,
    );

    Ok(Json(VisualizerStatusResponse {
        status: status.to_string(),
        stream_health: stream_health_label(stream_health).to_string(),
        live_visualizer_attached,
        restart_cooldown_active,
        consecutive_restart_failures: recovery.consecutive_restart_failures,
        healthy_ping_streak: recovery.healthy_ping_streak,
        auto_fallback_to_default_active: recovery.auto_fallback_to_default_active,
        current_audio_backend,
        device,
    }))
}

fn current_now_playing_snapshot(state: &ApiState) -> Result<NowPlayingResponse, ApiError> {
    let guard = state.now_playing.lock();
    Ok(guard.snapshot())
}

#[cfg(target_os = "macos")]
fn start_now_playing_reader(state: &ApiState) {
    let state = state.clone();
    thread::spawn(move || {
        use media_remote::{NowPlayingInfo, NowPlayingPerl, Subscription};
        use std::sync::RwLockReadGuard;

        let now_playing = NowPlayingPerl::new();
        let state_clone = state.clone();
        now_playing.subscribe(move |guard: RwLockReadGuard<'_, Option<NowPlayingInfo>>| {
            let Some(info) = guard.as_ref() else { return };

            {
                let mut np = state_clone.now_playing.lock();
                np.reader_running = true;
                np.last_error = None;
                np.updated_at_ms = Some(now_unix_ms());

                // Detect track change against previous state before overwriting.
                // On change, drop stale artwork-derived state so the old track's
                // colors don't linger if the new track arrives without a cover.
                let track_changed = np.track.title != info.title
                    || np.track.artist != info.artist
                    || np.track.album != info.album;

                np.track.title = info.title.clone();
                np.track.artist = info.artist.clone();
                np.track.album = info.album.clone();
                np.playback_state = match info.is_playing {
                    Some(true) => PlaybackState::Playing,
                    Some(false) => PlaybackState::Paused,
                    None => PlaybackState::Stopped,
                };
                if let (Some(elapsed), Some(duration)) = (info.elapsed_time, info.duration) {
                    let start = 0u64;
                    let current = (elapsed * 44100.0) as u64;
                    let end = (duration * 44100.0) as u64;
                    np.progress_rtp = Some((start, current, end));
                    np.progress_received_at_ms = Some(now_unix_ms());
                } else {
                    np.progress_rtp = None;
                    np.progress_received_at_ms = None;
                }
                np.track.duration_ms = info.duration.map(|d| (d * 1000.0) as u64);

                if track_changed {
                    np.palette_colors.clear();
                    np.artwork_bytes = None;
                    np.artwork_mime_type = None;
                    np.artwork_generation = np.artwork_generation.saturating_add(1);
                }

                if let Some(ref cover) = info.album_cover {
                    let mut buf: std::io::Cursor<Vec<u8>> = std::io::Cursor::new(Vec::new());
                    if cover.write_to(&mut buf, image::ImageFormat::Jpeg).is_ok() {
                        let bytes = buf.into_inner();
                        let colors = extract_prominent_colors(&bytes).unwrap_or_default();
                        np.artwork_mime_type = Some("image/jpeg".to_string());
                        np.artwork_bytes = Some(bytes);
                        np.artwork_generation = np.artwork_generation.saturating_add(1);
                        np.palette_colors = colors;
                    }
                }
            }

            // Re-evaluate idle/artwork state from the freshly-updated artwork
            // and playback fields. Decides whether to push SetIdleColor or
            // SetPalette to the visualizer.
            update_idle_state(&state_clone);
        });

        // Block to keep the subscriber alive
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    });
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn start_now_playing_reader(_state: &ApiState) {
    // No platform-specific now-playing source on Windows / other targets.
    // The API still serves cached snapshots; the reader is just a no-op.
}

#[cfg(target_os = "linux")]
fn start_now_playing_reader(state: &ApiState) {
    // Spawn the cover-art color-extraction worker once. The reader loop hands
    // PICT bytes off via this channel so it can keep draining the FIFO.
    let pict_tx = start_pict_worker(state.clone());
    let state = state.clone();
    thread::spawn(move || {
        loop {
            let metadata_pipe_path = state.now_playing.lock().metadata_pipe_path.clone();

            match OpenOptions::new().read(true).open(&metadata_pipe_path) {
                Ok(file) => {
                    {
                        let mut guard = state.now_playing.lock();
                        guard.reader_running = true;
                        guard.last_error = None;
                        guard.updated_at_ms = Some(now_unix_ms());
                    }

                    let reader = BufReader::new(file);
                    let result = process_shairport_metadata_stream(&state, reader, &pict_tx);

                    let mut guard = state.now_playing.lock();
                    guard.reader_running = false;
                    guard.updated_at_ms = Some(now_unix_ms());
                    if let Err(err) = &result {
                        guard.last_error = Some(err.clone());
                    }
                    if let Err(err) = result {
                        eprintln!("WARNING: metadata stream error: {}", err);
                    }
                }
                Err(err) => {
                    let mut guard = state.now_playing.lock();
                    guard.reader_running = false;
                    guard.last_error = Some(format!(
                        "Failed to open metadata pipe '{}': {}",
                        metadata_pipe_path, err
                    ));
                    guard.updated_at_ms = Some(now_unix_ms());
                }
            }

            thread::sleep(NOW_PLAYING_RETRY_DELAY);
        }
    });
}

fn start_live_visualizer_watchdog(state: &ApiState) {
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(LIVE_VISUALIZER_WATCHDOG_INTERVAL).await;
            if let Err(err) = run_live_visualizer_watchdog_tick(&state).await {
                eprintln!(
                    "WARNING: live visualizer watchdog tick failed: {}",
                    err.message
                );
            }
        }
    });
}

async fn run_live_visualizer_watchdog_tick(state: &ApiState) -> Result<(), ApiError> {
    let should_recover = match current_live_visualizer(state)? {
        Some(runtime) => runtime
            .sender
            .send(audioleaf::visualizer::VisualizerMsg::Ping)
            .is_err(),
        None => true,
    };
    if !should_recover {
        mark_live_visualizer_watchdog_healthy(state)?;
        return Ok(());
    }

    recover_live_visualizer(state, "watchdog health check").await
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
enum MetadataDisposition {
    /// Code is unknown / unhandled — don't even decode the base64 payload.
    Skip,
    /// Handler doesn't read the payload — dispatch with an empty Vec.
    NoPayload,
    /// Handler reads the payload — decode base64 and dispatch.
    Decode,
}

/// Decide whether an `<item>`'s payload is worth decoding before we hand it to
/// `apply_metadata_item_to_state`. Shairport emits dozens of DMAP codes per
/// burst; only this whitelisted subset is consumed. Skipping the rest avoids
/// per-item base64 alloc/decode that would otherwise stall the reader thread
/// (and backpressure the FIFO into shairport).
#[cfg(target_os = "linux")]
fn metadata_code_disposition(item_type: &str, code: &str) -> MetadataDisposition {
    match (item_type, code) {
        (
            "core",
            "minm" | "asar" | "asal" | "asgn" | "ascp" | "asul" | "astm" | "asdk" | "caps",
        ) => MetadataDisposition::Decode,
        ("ssnc", "snam" | "snua" | "clip" | "conn" | "PICT" | "prgr" | "pvol") => {
            MetadataDisposition::Decode
        }
        (
            "ssnc",
            "pbeg" | "prsm" | "pres" | "pfls" | "paus" | "pend" | "disc" | "mdst" | "mden" | "pcst"
            | "pcen",
        ) => MetadataDisposition::NoPayload,
        _ => MetadataDisposition::Skip,
    }
}

/// Cover-art job sent from the metadata reader thread to the dedicated PICT
/// worker thread. `generation` tracks `NowPlayingRuntimeState::artwork_generation`
/// at the time the job was enqueued; the worker drops its result if the
/// generation has advanced (i.e. a newer cover arrived first).
#[cfg(target_os = "linux")]
struct PictJob {
    generation: u64,
    bytes: Vec<u8>,
}

/// Spawn a single-purpose worker that runs `extract_prominent_colors_from_bytes`
/// off the metadata reader thread. The reader thread used to do this inline
/// inside the `now_playing` mutex critical section, which blocked both the FIFO
/// drain and `/api/now-playing` for the duration of the image work
/// (100–500 ms per cover on a Pi).
///
/// The worker uses a drain-to-latest pattern: when extraction finishes, any
/// queued newer jobs are skipped over so we always work on the freshest cover.
#[cfg(target_os = "linux")]
fn start_pict_worker(state: ApiState) -> flume::Sender<PictJob> {
    let (tx, rx) = flume::unbounded::<PictJob>();
    thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                job = newer;
            }

            let colors = audioleaf::now_playing::extract_prominent_colors_from_bytes(&job.bytes)
                .unwrap_or_default();

            let applied = {
                let mut guard = state.now_playing.lock();
                if guard.artwork_generation == job.generation {
                    guard.palette_colors = colors;
                    true
                } else {
                    false
                }
            };
            if applied {
                update_idle_state(&state);
            }
        }
    });
    tx
}

/// Shairport Sync writes a stream of `<item>` elements to the metadata pipe.
/// Each item looks like:
///   <item><type>636f7265</type><code>61736172</code><length>26</length>
///   <data encoding="base64">
///   RE1ORFMgJiBEYW5jZSBGcnVpdHMgTXVzaWM=</data></item>
/// Large payloads (cover art) span many base64 lines before `</data></item>`.
/// This parser uses quick-xml to handle all shapes robustly (inline or multi-line,
/// with or without `<data>`).
#[cfg(target_os = "linux")]
fn process_shairport_metadata_stream<R: BufRead>(
    state: &ApiState,
    reader: R,
    pict_tx: &flume::Sender<PictJob>,
) -> std::result::Result<(), String> {
    let mut xml = XmlReader::from_reader(reader);
    xml.config_mut().check_end_names = false;
    xml.config_mut().trim_text(true);

    #[derive(Default)]
    struct Cursor {
        in_item: bool,
        current_tag: Option<String>,
        type_hex: String,
        code_hex: String,
        length: usize,
        encoded: String,
        has_data: bool,
    }
    let mut cur = Cursor::default();
    let mut buf: Vec<u8> = Vec::new();

    loop {
        buf.clear();
        match xml.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "item" => {
                        cur = Cursor {
                            in_item: true,
                            ..Cursor::default()
                        }
                    }
                    "type" | "code" | "length" | "data" if cur.in_item => {
                        cur.current_tag = Some(name.clone());
                        if name == "data" {
                            cur.has_data = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::Text(e)) => {
                if !cur.in_item {
                    continue;
                }
                // Shairport emits hex (type/code/length) and base64 (data) only,
                // so plain UTF-8 decode is sufficient; no XML-entity unescaping needed.
                let text = std::str::from_utf8(e.as_ref()).unwrap_or("");
                match cur.current_tag.as_deref() {
                    Some("type") => cur.type_hex.push_str(text.trim()),
                    Some("code") => cur.code_hex.push_str(text.trim()),
                    Some("length") => {
                        cur.length = text.trim().parse().unwrap_or(0);
                    }
                    Some("data") => {
                        for part in text.split_whitespace() {
                            cur.encoded.push_str(part);
                        }
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::CData(e)) => {
                if cur.current_tag.as_deref() == Some("data") {
                    let bytes = e.into_inner();
                    for part in std::str::from_utf8(&bytes).unwrap_or("").split_whitespace() {
                        cur.encoded.push_str(part);
                    }
                }
            }
            Ok(XmlEvent::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "item" {
                    if !cur.type_hex.is_empty() && !cur.code_hex.is_empty() {
                        let item_type = decode_fourcc(&cur.type_hex)
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_default();
                        let code = decode_fourcc(&cur.code_hex).unwrap_or_default();
                        if !item_type.is_empty() && !code.is_empty() {
                            match metadata_code_disposition(&item_type, &code) {
                                MetadataDisposition::Skip => {}
                                MetadataDisposition::NoPayload => {
                                    apply_metadata_item_to_state(
                                        state,
                                        &item_type,
                                        &code,
                                        Vec::new(),
                                        pict_tx,
                                    );
                                }
                                MetadataDisposition::Decode => {
                                    let payload = if cur.has_data && !cur.encoded.is_empty() {
                                        base64::engine::general_purpose::STANDARD
                                            .decode(cur.encoded.as_bytes())
                                            .unwrap_or_default()
                                    } else {
                                        Vec::new()
                                    };
                                    apply_metadata_item_to_state(
                                        state, &item_type, &code, payload, pict_tx,
                                    );
                                }
                            }
                        }
                    }
                    cur = Cursor::default();
                } else if cur.current_tag.as_deref() == Some(name.as_str()) {
                    cur.current_tag = None;
                }
            }
            Ok(XmlEvent::Eof) => return Ok(()),
            Ok(_) => {}
            Err(err) => {
                // Non-fatal: the pipe can be joined mid-stream, producing
                // orphaned close tags. Reset cursor and keep reading.
                eprintln!("WARNING: metadata XML parse error (continuing): {err}");
                cur = Cursor::default();
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_metadata_item_to_state(
    state: &ApiState,
    item_type: &str,
    code: &str,
    payload: Vec<u8>,
    pict_tx: &flume::Sender<PictJob>,
) {
    let payload_text = payload_bytes_to_string(&payload);

    let mut guard = state.now_playing.lock();
    guard.reader_running = true;
    guard.last_error = None;
    guard.updated_at_ms = Some(now_unix_ms());

    if metadata_logging_enabled() {
        log_metadata_event(
            &format!("{}/{} len={}", item_type, code, payload.len()),
            Some(&payload),
        );
    }

    match item_type {
        "core" => match code {
            "minm" => guard.track.title = payload_text,
            "asar" => guard.track.artist = payload_text,
            "asal" => guard.track.album = payload_text,
            "asgn" => guard.track.genre = payload_text,
            "ascp" => guard.track.composer = payload_text,
            "asul" => guard.track.stream_url = payload_text,
            "astm" => guard.track.duration_ms = dmap_payload_u64(&payload),
            "asdk" => guard.track.song_data_kind = dmap_payload_u32(&payload),
            // caps = DACP player state byte.
            // Mapping varies across implementations (some use 3=playing, others 4=playing).
            // Treat any value >= 2 as "active" — rely on prsm/pfls/paus for play vs pause.
            "caps" => {
                if let Some(v) = dmap_payload_u32(&payload) {
                    if v >= 2 {
                        // Don't downgrade from Playing to Paused based on caps alone —
                        // only prsm/pfls/paus should toggle between those states.
                        if guard.playback_state != PlaybackState::Playing {
                            guard.playback_state = PlaybackState::Playing;
                        }
                    } else {
                        guard.playback_state = PlaybackState::Stopped;
                    }
                }
            }
            _ => {}
        },
        "ssnc" => match code {
            "snam" => guard.track.source_name = payload_text,
            "snua" => guard.track.user_agent = payload_text,
            "clip" | "conn" => guard.track.source_ip = payload_text,
            "PICT" if !payload.is_empty() => {
                guard.artwork_mime_type = detect_image_mime_type(&payload).map(str::to_string);
                let new_generation = guard.artwork_generation.saturating_add(1);
                guard.artwork_generation = new_generation;
                // Clone for the worker; the state retains its own copy via
                // `artwork_bytes` so /api/now-playing/artwork serves immediately.
                // palette_colors is intentionally NOT cleared — old colors stay
                // visible until the worker computes the new palette.
                let job_bytes = payload.clone();
                guard.artwork_bytes = Some(payload);
                let _ = pict_tx.send(PictJob {
                    generation: new_generation,
                    bytes: job_bytes,
                });
            }
            // Playback state transitions
            "pbeg" => {
                guard.clear_session_data();
                guard.playback_state = PlaybackState::Playing;
            }
            "prsm" | "pres" => guard.playback_state = PlaybackState::Playing,
            "pfls" | "paus" => guard.playback_state = PlaybackState::Paused,
            "pend" | "disc" => {
                guard.playback_state = PlaybackState::Stopped;
                guard.clear_session_data();
            }
            // Progress: "start/current/end" RTP timestamps at 44100 Hz.
            // shairport sends this at stream start and on flush events; we
            // anchor wall-clock here so the snapshot can extrapolate elapsed.
            "prgr" => {
                let parsed = parse_prgr(&payload);
                guard.progress_rtp = parsed;
                if parsed.is_some() {
                    guard.progress_received_at_ms = Some(now_unix_ms());
                    guard.playback_state = PlaybackState::Playing;
                } else {
                    guard.progress_received_at_ms = None;
                    log_metadata_event("WARNING: failed to parse prgr payload", Some(&payload));
                }
            }
            // Volume: "airplay_vol,actual_vol,lowest,highest" in dB
            "pvol" => guard.volume_db = parse_pvol(&payload),
            // Metadata bundle boundaries (informational — no action needed)
            "mdst" | "mden" | "pcst" | "pcen" => {}
            _ => {}
        },
        _ => {}
    }

    drop(guard);

    // Idle/artwork state may have changed (PICT, playback transition, prgr).
    // Recompute and emit SetIdleColor / SetPalette if needed.
    update_idle_state(state);
}

#[cfg(target_os = "linux")]
fn dmap_payload_u32(payload: &[u8]) -> Option<u32> {
    match payload.len() {
        1 => Some(payload[0] as u32),
        2 => Some(u16::from_be_bytes([payload[0], payload[1]]) as u32),
        4 => Some(u32::from_be_bytes([
            payload[0], payload[1], payload[2], payload[3],
        ])),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn dmap_payload_u64(payload: &[u8]) -> Option<u64> {
    match payload.len() {
        1..=4 => dmap_payload_u32(payload).map(|v| v as u64),
        8 => Some(u64::from_be_bytes(payload[..8].try_into().ok()?)),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn parse_prgr(payload: &[u8]) -> Option<(u64, u64, u64)> {
    let text = std::str::from_utf8(payload).ok()?;
    let mut parts = text.split('/');
    let start = parts.next()?.trim().parse().ok()?;
    let current = parts.next()?.trim().parse().ok()?;
    let end = parts.next()?.trim().parse().ok()?;
    Some((start, current, end))
}

#[cfg(target_os = "linux")]
fn parse_pvol(payload: &[u8]) -> Option<f32> {
    let text = std::str::from_utf8(payload).ok()?;
    let first = text.split(',').next()?.trim();
    first.parse().ok()
}

#[cfg(target_os = "linux")]
fn payload_bytes_to_string(payload: &[u8]) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    let value = String::from_utf8_lossy(payload)
        .trim_matches('\0')
        .trim()
        .to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(target_os = "linux")]
fn decode_fourcc(hex_value: &str) -> Option<String> {
    let raw = u32::from_str_radix(hex_value, 16).ok()?.to_be_bytes();
    Some(raw.iter().map(|byte| *byte as char).collect())
}

#[cfg(target_os = "linux")]
fn detect_image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // BMP
    if bytes.starts_with(b"BM") && bytes.len() > 14 {
        return Some("image/bmp");
    }
    // TIFF (little-endian II or big-endian MM)
    if bytes.len() >= 4
        && ((bytes[0..2] == [0x49, 0x49] && bytes[2..4] == [0x2A, 0x00])
            || (bytes[0..2] == [0x4D, 0x4D] && bytes[2..4] == [0x00, 0x2A]))
    {
        return Some("image/tiff");
    }
    // GIF
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    None
}

#[cfg(target_os = "macos")]
fn extract_prominent_colors(image_bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
    audioleaf::now_playing::extract_prominent_colors_from_bytes(image_bytes)
}

fn now_unix_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}

/// True when AUDIOLEAF_LOG_METADATA is set to a truthy value. Cached on first
/// read — toggling at runtime requires a process restart.
#[allow(dead_code)] // Only consumed by the Linux shairport-metadata path.
fn metadata_logging_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("AUDIOLEAF_LOG_METADATA")
            .ok()
            .map(|v| !matches!(v.as_str(), "" | "0" | "false" | "no"))
            .unwrap_or(false)
    })
}

/// Log an ad-hoc metadata-stream event. Rendering a preview of the payload
/// (UTF-8 if printable, hex-bytes otherwise, truncated for large blobs).
/// No-op unless AUDIOLEAF_LOG_METADATA is set.
#[allow(dead_code)] // Only consumed by the Linux shairport-metadata path.
fn log_metadata_event(message: &str, payload: Option<&[u8]>) {
    if !metadata_logging_enabled() {
        return;
    }
    let preview = match payload {
        None => String::new(),
        Some([]) => " (empty)".to_string(),
        Some(bytes) => {
            let utf8 = std::str::from_utf8(bytes)
                .ok()
                .map(|s| s.trim_matches('\0'))
                .filter(|s| s.chars().all(|c| !c.is_control() || c == '\n' || c == '\t'));
            if let Some(text) = utf8 {
                format!(" \"{}\"", text)
            } else if bytes.len() <= 16 {
                format!(" bytes={:02x?}", bytes)
            } else {
                format!(" <{} bytes>", bytes.len())
            }
        }
    };
    eprintln!("META {}{}", message, preview);
}

fn resolve_paths(state: &ApiState) -> Result<PathsResponse, ApiError> {
    let ((config_file_path, config_file_exists), (devices_file_path, devices_file_exists)) =
        audioleaf::config::resolve_paths(
            state.config_file_path.clone(),
            state.devices_file_path.clone(),
        )
        .map_err(ApiError::internal)?;

    Ok(PathsResponse {
        config_file_path: config_file_path.to_string_lossy().into_owned(),
        config_file_exists,
        devices_file_path: devices_file_path.to_string_lossy().into_owned(),
        devices_file_exists,
    })
}

fn load_device_by_name(
    state: &ApiState,
    name: &str,
) -> Result<audioleaf::nanoleaf::NlDevice, ApiError> {
    let paths = resolve_paths(state)?;
    if !paths.devices_file_exists {
        return Err(ApiError::not_found(format!(
            "No devices file found at {}",
            paths.devices_file_path
        )));
    }
    let devices_path = PathBuf::from(&paths.devices_file_path);
    audioleaf::nanoleaf::NlDevice::find_in_file(&devices_path, Some(name))
        .map_err(|err| ApiError::not_found(err.to_string()))
}

fn get_runtime_config_clone(state: &ApiState) -> Result<audioleaf::config::Config, ApiError> {
    let guard = state.runtime_config.lock();
    Ok(guard.clone())
}

/// Resolve the `NlDevice` we should pull palettes from. Uses
/// `default_nl_device_name` from config if set; otherwise the first device in
/// `nl_devices.toml`.
fn active_palette_device(state: &ApiState) -> Result<audioleaf::nanoleaf::NlDevice, ApiError> {
    let config = get_runtime_config_clone(state)?;
    let paths = resolve_paths(state)?;
    if !paths.devices_file_exists {
        return Err(ApiError::not_found(format!(
            "No devices file found at {}",
            paths.devices_file_path
        )));
    }
    let devices_path = PathBuf::from(&paths.devices_file_path);
    let preferred = config.default_nl_device_name.as_deref();
    audioleaf::nanoleaf::NlDevice::find_in_file(&devices_path, preferred)
        .map_err(|err| ApiError::not_found(err.to_string()))
}

/// Read palettes from cache. Refreshes from the device if missing or stale.
/// Returns an empty Vec on device-fetch failure rather than failing the
/// caller — palette lookups should degrade gracefully when the device is off.
fn palettes_cached(state: &ApiState) -> Vec<audioleaf::nanoleaf::NamedPalette> {
    {
        let guard = state.palette_cache.lock();
        if let Some((fetched_at, palettes)) = guard.as_ref()
            && fetched_at.elapsed() < PALETTE_CACHE_TTL
        {
            return palettes.clone();
        }
    }
    let device = match active_palette_device(state) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    match device.list_effect_palettes() {
        Ok(palettes) => {
            let mut guard = state.palette_cache.lock();
            *guard = Some((Instant::now(), palettes.clone()));
            palettes
        }
        Err(err) => {
            eprintln!(
                "WARNING: failed to fetch palettes from Nanoleaf device '{}': {}",
                device.name, err
            );
            Vec::new()
        }
    }
}

/// Resolve a palette name to its colors. Falls back to the device's
/// currently-selected effect when:
///     - the requested name doesn't exist (renamed/removed in the Nanoleaf app)
///     - no name was given (`name = None`)
/// Final fallback if nothing matches: `constants::DEFAULT_COLORS`, so the
/// visualizer always has something to animate.
fn resolve_palette_colors(state: &ApiState, name: Option<&str>) -> Vec<[u8; 3]> {
    let palettes = palettes_cached(state);

    if let Some(want) = name
        && let Some(found) = palettes.iter().find(|p| p.name == want)
    {
        return found.colors.clone();
    }

    // Try the device's currently-selected effect as the fallback.
    if let Ok(device) = active_palette_device(state)
        && let Ok(info) = device.get_device_info()
        && let Some(selected) = info["effects"]["select"].as_str()
        && let Some(found) = palettes.iter().find(|p| p.name == selected)
    {
        return found.colors.clone();
    }

    // Last resort: hard-coded gradient, used at startup if device is offline.
    Vec::from(audioleaf::constants::DEFAULT_COLORS)
}

/// Async wrapper around [`palettes_cached`] for use from `async fn` handlers.
/// The underlying call hits the Nanoleaf via blocking reqwest — without
/// `spawn_blocking` it would pin a tokio worker for the duration of the
/// device round-trip (often seconds when the Pi/Nanoleaf are slow), and
/// enough of those at once will starve the runtime.
async fn palettes_cached_async(state: &ApiState) -> Vec<audioleaf::nanoleaf::NamedPalette> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || palettes_cached(&state))
        .await
        .unwrap_or_default()
}

/// Async wrapper around [`resolve_palette_colors`]. Same rationale.
async fn resolve_palette_colors_async(state: &ApiState, name: Option<String>) -> Vec<[u8; 3]> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || resolve_palette_colors(&state, name.as_deref()))
        .await
        .unwrap_or_default()
}

/// Async wrapper around [`update_idle_state`]. Errors are already swallowed
/// inside; we just need to keep the blocking work off tokio workers.
async fn update_idle_state_async(state: &ApiState) {
    let state = state.clone();
    let _ = tokio::task::spawn_blocking(move || update_idle_state(&state)).await;
}

/// Compute whether the visualizer should be in "idle white" state right now,
/// and apply the transition (send SetIdleColor / SetPalette as appropriate).
/// Idle when: color_source = Artwork AND no artwork colors AND not playing.
fn update_idle_state(state: &ApiState) {
    let config = match get_runtime_config_clone(state) {
        Ok(c) => c,
        Err(_) => return,
    };
    let is_artwork_mode = matches!(
        config.visualizer_config.color_source,
        Some(audioleaf::config::ColorSourceKind::Artwork)
    );

    let (artwork_present, is_playing) = {
        let guard = state.now_playing.lock();
        (
            !guard.palette_colors.is_empty(),
            matches!(guard.playback_state, PlaybackState::Playing),
        )
    };

    let should_idle = is_artwork_mode && (!artwork_present || !is_playing);

    let Ok(Some(runtime)) = current_live_visualizer(state) else {
        return;
    };
    if should_idle {
        let _ = runtime
            .sender
            .send(audioleaf::visualizer::VisualizerMsg::SetIdleColor(Some(
                audioleaf::constants::IDLE_WHITE_RGB,
            )));
    } else {
        // Exit idle: clear the flag and re-apply whichever palette is current
        // so the audio pipeline has the right base colors to drive.
        let _ = runtime
            .sender
            .send(audioleaf::visualizer::VisualizerMsg::SetIdleColor(None));
        let colors = if is_artwork_mode {
            // Use artwork colors (we know it's present from the check above)
            state.now_playing.lock().palette_colors.clone()
        } else {
            resolve_palette_colors(state, config.visualizer_config.palette_name.as_deref())
        };
        if !colors.is_empty() {
            let _ = runtime
                .sender
                .send(audioleaf::visualizer::VisualizerMsg::SetPalette(colors));
        }
    }
}

fn update_runtime_config<F>(
    state: &ApiState,
    updater: F,
) -> Result<audioleaf::config::Config, ApiError>
where
    F: FnOnce(&mut audioleaf::config::Config),
{
    let mut guard = state.runtime_config.lock();
    updater(&mut guard);
    Ok(guard.clone())
}

fn current_live_visualizer(state: &ApiState) -> Result<Option<LiveVisualizerRuntime>, ApiError> {
    let guard = state.live_visualizer.lock();
    Ok(guard.clone())
}

async fn ensure_live_visualizer(state: &ApiState) -> Result<LiveVisualizerRuntime, ApiError> {
    if let Some(runtime) = current_live_visualizer(state)? {
        return Ok(runtime);
    }

    recover_live_visualizer(state, "ensure_live_visualizer").await?;
    current_live_visualizer(state)?
        .ok_or_else(|| ApiError::internal("Live visualizer failed to initialize"))
}

fn mark_live_visualizer_recovery_success(
    state: &ApiState,
    auto_fallback_to_default_active: bool,
) -> Result<(), ApiError> {
    let mut guard = state.live_visualizer_recovery.lock();
    guard.auto_fallback_to_default_active = auto_fallback_to_default_active;
    guard.last_restart_at_ms = Some(now_unix_ms());
    guard.healthy_ping_streak = 0;
    Ok(())
}

fn mark_live_visualizer_recovery_failure(state: &ApiState) -> Result<u32, ApiError> {
    let mut guard = state.live_visualizer_recovery.lock();
    guard.consecutive_restart_failures = guard.consecutive_restart_failures.saturating_add(1);
    guard.healthy_ping_streak = 0;
    Ok(guard.consecutive_restart_failures)
}

fn mark_live_visualizer_restart_attempt(state: &ApiState) -> Result<(), ApiError> {
    let mut guard = state.live_visualizer_recovery.lock();
    guard.last_restart_at_ms = Some(now_unix_ms());
    guard.healthy_ping_streak = 0;
    Ok(())
}

fn mark_live_visualizer_watchdog_healthy(state: &ApiState) -> Result<(), ApiError> {
    let mut guard = state.live_visualizer_recovery.lock();

    if guard.consecutive_restart_failures == 0 {
        guard.healthy_ping_streak = 0;
        return Ok(());
    }

    guard.healthy_ping_streak = guard.healthy_ping_streak.saturating_add(1);
    if guard.healthy_ping_streak >= LIVE_VISUALIZER_HEALTHY_PINGS_TO_CLEAR_FAILURES {
        guard.consecutive_restart_failures = 0;
        guard.healthy_ping_streak = 0;
        eprintln!(
            "INFO: cleared live visualizer restart failure counter after {} healthy watchdog pings.",
            LIVE_VISUALIZER_HEALTHY_PINGS_TO_CLEAR_FAILURES
        );
    }
    Ok(())
}

fn live_visualizer_restart_cooldown_remaining(
    state: &ApiState,
) -> Result<Option<Duration>, ApiError> {
    let guard = state.live_visualizer_recovery.lock();

    let Some(last_restart_at_ms) = guard.last_restart_at_ms else {
        return Ok(None);
    };
    let base_ms = LIVE_VISUALIZER_RESTART_COOLDOWN.as_millis() as u64;
    let max_ms = LIVE_VISUALIZER_RESTART_COOLDOWN_MAX.as_millis() as u64;
    let shift = guard.consecutive_restart_failures.min(20);
    let cooldown_ms = base_ms.checked_shl(shift).unwrap_or(max_ms).min(max_ms);
    let now_ms = now_unix_ms();
    let elapsed_ms = now_ms.saturating_sub(last_restart_at_ms);
    if elapsed_ms >= cooldown_ms {
        Ok(None)
    } else {
        Ok(Some(Duration::from_millis(cooldown_ms - elapsed_ms)))
    }
}

async fn recover_live_visualizer(state: &ApiState, reason: &str) -> Result<(), ApiError> {
    if let Some(delay) = live_visualizer_restart_cooldown_remaining(state)? {
        tokio::time::sleep(delay).await;
    }
    mark_live_visualizer_restart_attempt(state)?;

    let configured_backend = get_runtime_config_clone(state)?
        .visualizer_config
        .audio_backend
        .unwrap_or_else(|| audioleaf::constants::DEFAULT_AUDIO_BACKEND.to_string());

    match restart_live_visualizer(state).await {
        Ok(()) => {
            mark_live_visualizer_recovery_success(state, false)?;
            return Ok(());
        }
        Err(primary_err) => {
            let failure_count = mark_live_visualizer_recovery_failure(state)?;
            eprintln!(
                "WARNING: live visualizer restart failed (reason: {}, backend: {}, consecutive_failures: {}): {}",
                reason, configured_backend, failure_count, primary_err.message
            );

            let should_try_default_fallback = configured_backend
                != audioleaf::constants::DEFAULT_AUDIO_BACKEND
                && failure_count >= LIVE_VISUALIZER_RESTART_FAILURE_LIMIT;
            if !should_try_default_fallback {
                return Err(primary_err);
            }
        }
    }

    eprintln!(
        "WARNING: falling back live visualizer backend to '{}' after repeated restart failures.",
        audioleaf::constants::DEFAULT_AUDIO_BACKEND
    );
    update_runtime_config(state, |config| {
        config.visualizer_config.audio_backend =
            Some(audioleaf::constants::DEFAULT_AUDIO_BACKEND.to_string());
    })?;
    mark_live_visualizer_restart_attempt(state)?;
    restart_live_visualizer(state).await?;
    mark_live_visualizer_recovery_success(state, true)?;
    Ok(())
}

async fn send_live_message_with_recovery(
    state: &ApiState,
    message: audioleaf::visualizer::VisualizerMsg,
) -> Result<(), ApiError> {
    let live = ensure_live_visualizer(state).await?;
    if live.sender.send(message.clone()).is_ok() {
        return Ok(());
    }

    recover_live_visualizer(state, "control message send failure").await?;
    let restarted = ensure_live_visualizer(state).await?;
    restarted
        .sender
        .send(message)
        .map_err(|_| ApiError::internal("Failed to send command to live visualizer"))
}

async fn restart_live_visualizer(state: &ApiState) -> Result<(), ApiError> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || restart_live_visualizer_sync(&state))
        .await
        .map_err(handle_join_error)?
}

fn restart_live_visualizer_sync(state: &ApiState) -> Result<(), ApiError> {
    let new_runtime = build_live_visualizer(state)?;
    let old_runtime = {
        let mut guard = state.live_visualizer.lock();
        guard.replace(new_runtime)
    };

    if let Some(old_runtime) = old_runtime {
        let _ = old_runtime
            .sender
            .send(audioleaf::visualizer::VisualizerMsg::End);
    }
    Ok(())
}

fn build_live_visualizer(state: &ApiState) -> Result<LiveVisualizerRuntime, ApiError> {
    let config = get_runtime_config_clone(state)?;
    let paths = resolve_paths(state)?;
    if !paths.devices_file_exists {
        return Err(ApiError::not_found(format!(
            "No devices file found at {}",
            paths.devices_file_path
        )));
    }

    let devices_path = PathBuf::from(&paths.devices_file_path);
    let known_devices =
        audioleaf::nanoleaf::NlDevice::all_from_file(&devices_path).map_err(ApiError::internal)?;
    if known_devices.is_empty() {
        return Err(ApiError::not_found(format!(
            "No Nanoleaf devices found in {}",
            paths.devices_file_path
        )));
    }

    let preferred_name = config.default_nl_device_name.clone();
    let nl_device = if let Some(default_name) = preferred_name.as_deref() {
        match known_devices
            .iter()
            .find(|device| device.name == default_name)
        {
            Some(device) => device.clone(),
            None => {
                let fallback = known_devices[0].clone();
                eprintln!(
                    "WARNING: default_nl_device_name '{}' not found. Falling back to '{}'.",
                    default_name, fallback.name
                );
                fallback
            }
        }
    } else {
        known_devices[0].clone()
    };

    nl_device
        .ensure_device_ready()
        .map_err(ApiError::internal)?;
    nl_device
        .request_udp_control()
        .map_err(ApiError::internal)?;

    let global_orientation = nl_device
        .get_global_orientation()
        .ok()
        .and_then(|orientation| orientation["value"].as_u64())
        .unwrap_or(0) as u16;

    let configured_backend = config.visualizer_config.audio_backend.clone();
    let audio_stream = match audioleaf::audio::AudioStream::new(configured_backend.as_deref()) {
        Ok(stream) => stream,
        Err(primary_err) => {
            let should_try_default = configured_backend
                .as_deref()
                .is_some_and(|name| name != audioleaf::constants::DEFAULT_AUDIO_BACKEND);
            if !should_try_default {
                return Err(ApiError::internal(primary_err));
            }

            eprintln!(
                "WARNING: Failed to initialize audio backend '{}': {}. Falling back to '{}'.",
                configured_backend.as_deref().unwrap_or("unknown"),
                primary_err,
                audioleaf::constants::DEFAULT_AUDIO_BACKEND
            );
            audioleaf::audio::AudioStream::new(Some(audioleaf::constants::DEFAULT_AUDIO_BACKEND))
                .map_err(ApiError::internal)?
        }
    };

    let (color_tx, color_rx) = flume::bounded(1);
    let stream_health = Arc::new(Mutex::new(audioleaf::visualizer::StreamHealth::Starting));

    // Resolve initial hues from the configured color source. For Artwork mode
    // and any case where the palette can't be looked up, the visualizer's
    // own fallback (DEFAULT_COLORS) kicks in via empty input.
    let initial_hues = match config.visualizer_config.color_source {
        Some(audioleaf::config::ColorSourceKind::Artwork) => {
            // Use any current artwork colors; otherwise an empty Vec triggers
            // the visualizer's own DEFAULT_COLORS fallback (panels go to idle
            // white shortly after, via update_idle_state).
            state.now_playing.lock().palette_colors.clone()
        }
        _ => resolve_palette_colors(state, config.visualizer_config.palette_name.as_deref()),
    };

    let visualizer = audioleaf::visualizer::Visualizer::new(
        config.visualizer_config,
        audio_stream,
        &nl_device,
        vec![color_tx],
        initial_hues,
    )
    .map_err(ApiError::internal)?
    .with_stream_health(Arc::clone(&stream_health));
    let sender = visualizer.init();

    println!(
        "Live visualizer attached to '{}' at {}",
        nl_device.name, nl_device.ip
    );

    Ok(LiveVisualizerRuntime {
        sender,
        global_orientation,
        device: DeviceSummary {
            name: nl_device.name,
            ip: nl_device.ip.to_string(),
        },
        color_rx,
        latest_colors: Arc::new(Mutex::new(HashMap::new())),
        stream_health,
    })
}

fn parse_axis(input: &str) -> Option<audioleaf::config::Axis> {
    if input.eq_ignore_ascii_case("x") {
        Some(audioleaf::config::Axis::X)
    } else if input.eq_ignore_ascii_case("y") {
        Some(audioleaf::config::Axis::Y)
    } else {
        None
    }
}

fn parse_sort(input: &str) -> Option<audioleaf::config::Sort> {
    if input.eq_ignore_ascii_case("asc") {
        Some(audioleaf::config::Sort::Asc)
    } else if input.eq_ignore_ascii_case("desc") {
        Some(audioleaf::config::Sort::Desc)
    } else {
        None
    }
}

fn parse_effect(input: &str) -> Option<audioleaf::config::Effect> {
    match input {
        x if x.eq_ignore_ascii_case("spectrum") => Some(audioleaf::config::Effect::Spectrum),
        x if x.eq_ignore_ascii_case("energywave")
            || x.eq_ignore_ascii_case("energy_wave")
            || x.eq_ignore_ascii_case("energy-wave") =>
        {
            Some(audioleaf::config::Effect::EnergyWave)
        }
        x if x.eq_ignore_ascii_case("ripple") => Some(audioleaf::config::Effect::Ripple),
        _ => None,
    }
}

fn stream_health_label(stream_health: audioleaf::visualizer::StreamHealth) -> &'static str {
    match stream_health {
        audioleaf::visualizer::StreamHealth::Starting => "Starting",
        audioleaf::visualizer::StreamHealth::Healthy => "Healthy",
        audioleaf::visualizer::StreamHealth::Degraded => "Degraded",
        audioleaf::visualizer::StreamHealth::Restarting => "Restarting",
        audioleaf::visualizer::StreamHealth::Stopped => "Stopped",
    }
}

fn summarize_visualizer_status(
    live_visualizer_attached: bool,
    stream_health: audioleaf::visualizer::StreamHealth,
    consecutive_restart_failures: u32,
) -> &'static str {
    if !live_visualizer_attached {
        return "Restarting";
    }

    match stream_health {
        audioleaf::visualizer::StreamHealth::Healthy => {
            if consecutive_restart_failures > 0 {
                "Degraded"
            } else {
                "Healthy"
            }
        }
        audioleaf::visualizer::StreamHealth::Degraded => "Degraded",
        audioleaf::visualizer::StreamHealth::Starting
        | audioleaf::visualizer::StreamHealth::Restarting
        | audioleaf::visualizer::StreamHealth::Stopped => "Restarting",
    }
}

async fn run_nanoleaf_io<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(handle_join_error)?
        .map_err(ApiError::internal)
}

fn handle_join_error(err: JoinError) -> ApiError {
    ApiError::internal(format!("Background I/O task failed: {err}"))
}
