use ratatui::crossterm::terminal::LeaveAlternateScreen;
use ratatui::crossterm::{execute, terminal};

use crate::{constants, utils};
use std::io::{stdout, Write};
use std::{backtrace, fs};

pub fn register_backtrace_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        println!("audioleaf crashed unexpectedly");
        if let Ok(path) = utils::get_default_cache_dir() {
            let path = path.join(constants::DEFAULT_BACKTRACE_FILE);
            if let Ok(mut file) = fs::File::create(&path) {
                writeln!(file, "{}", backtrace::Backtrace::force_capture()).unwrap_or_default();
                writeln!(file, "{}", panic_info).unwrap_or_default();
                println!("The backtrace has been saved to {}", path.to_string_lossy());
            }
        }
    }));
}
