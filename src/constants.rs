// visualizer config
pub const DEFAULT_AUDIO_BACKEND: &str = "default";
pub const DEFAULT_FREQ_RANGE: (u16, u16) = (20, 4500);
pub const DEFAULT_COLORS: [[u8; 3]; 12] = [
    [255, 213, 0], // golden
    [255, 128, 0], // orange
    [255, 43, 0],  // scarlet
    [255, 0, 43],  // rose
    [255, 0, 128], // hot pink
    [255, 0, 213], // fuchsia
    [213, 0, 255], // purple
    [128, 0, 255], // violet
    [43, 0, 255],  // indigo
    [0, 43, 255],  // cobalt
    [0, 128, 255], // azure
    [0, 213, 255], // sky blue
];
pub const DEFAULT_GAIN: f32 = 1.0;
// Transition time in units of 100ms (0 = instant, 1 = 100ms, 2 = 200ms, etc.)
// Recommended: Use values that align with your time_window for smooth transitions
pub const DEFAULT_TRANSITION_TIME: u16 = 2;
pub const DEFAULT_TIME_WINDOW: f32 = 0.1875;

// other
pub const DEFAULT_CONFIG_DIR: &str = "audioleaf";
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
pub const DEFAULT_DEVICES_FILE: &str = "nl_devices.toml";
pub const DEFAULT_BACKTRACE_FILE: &str = "audioleaf_backtrace.log";
pub const NL_API_PORT: u16 = 16021;
pub const NL_UDP_PORT: u16 = 60222;
