// tui config
pub const DEFAULT_COLORFUL_EFFECT_NAMES: bool = false;

// visualizer config
pub const DEFAULT_AUDIO_BACKEND: &str = "default";
pub const DEFAULT_FREQ_RANGE: (u16, u16) = (20, 4500);
pub const DEFAULT_HUES: [u16; 12] = [50, 30, 10, 350, 330, 310, 290, 270, 250, 230, 210, 190];
pub const DEFAULT_GAIN: f32 = 1.0;
pub const DEFAULT_TRANSITION_TIME: u16 = 2;
pub const DEFAULT_TIME_WINDOW: f32 = 0.1875;

// other
pub const DEFAULT_CONFIG_DIR: &str = "audioleaf";
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";
pub const DEFAULT_DEVICES_FILE: &str = "nl_devices.toml";
pub const DEFAULT_BACKTRACE_FILE: &str = "audioleaf_backtrace.log";
pub const NL_API_PORT: u16 = 16021;
pub const NL_UDP_PORT: u16 = 60222;
pub const TICKRATE: u64 = 32;
