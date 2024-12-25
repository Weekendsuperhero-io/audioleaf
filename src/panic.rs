use crate::{constants, utils};
use std::io::Write;
use std::{backtrace, fs};

// inspired by hrkfdn/ncspot
pub fn register_backtrace_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        if let Ok(path) = utils::get_default_cache_dir() {
            let path = path.join(constants::DEFAULT_BACKTRACE_FILE);
            if let Ok(mut file) = fs::File::create(path) {
                writeln!(file, "{}", backtrace::Backtrace::force_capture()).unwrap_or_default();
                writeln!(file, "{}", panic_info).unwrap_or_default();
            }
        }
    }));
}
