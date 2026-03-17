use crate::constants;
use ratatui::crossterm::{execute, terminal};
use std::{
    backtrace, fs,
    io::{Write, stdout},
};

/// Registers a custom panic hook to handle application crashes gracefully.
///
/// Disables raw mode and leaves alternate screen before printing crash message.
/// Captures and saves backtrace to cache/audioleaf_backtrace.log if possible.
/// Ensures TUI state is restored on panic in threads like event handler.
pub fn register_backtrace_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(stdout(), terminal::LeaveAlternateScreen);
        println!("Audioleaf crashed unexpectedly!");
        if let Some(path) = dirs::cache_dir() {
            let path = path.join(constants::DEFAULT_BACKTRACE_FILE);
            if let Ok(mut file) = fs::File::create(&path) {
                writeln!(file, "{}", backtrace::Backtrace::force_capture()).unwrap_or_default();
                writeln!(file, "{}", panic_info).unwrap_or_default();
                println!("The backtrace has been saved to {}", path.to_string_lossy());
            }
        }
    }));
}
