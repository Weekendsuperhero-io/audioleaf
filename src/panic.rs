use crate::constants;
use std::{backtrace, fs, io::Write};

/// Registers a custom panic hook to handle application crashes gracefully.
///
/// Captures and saves backtrace to cache/audioleaf_backtrace.log if possible.
pub fn register_backtrace_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("Audioleaf crashed unexpectedly!");
        if let Some(path) = dirs::cache_dir() {
            let path = path.join(constants::DEFAULT_BACKTRACE_FILE);
            if let Ok(mut file) = fs::File::create(&path) {
                writeln!(file, "{}", backtrace::Backtrace::force_capture()).unwrap_or_default();
                writeln!(file, "{}", panic_info).unwrap_or_default();
                eprintln!("The backtrace has been saved to {}", path.to_string_lossy());
            }
        }
    }));
}
