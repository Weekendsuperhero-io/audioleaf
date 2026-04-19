//! Album art color extraction for the visualizer.
//!
//! macOS: Uses ScriptingBridge.framework (via objc2) to query running media
//! players directly through the ObjC bridge — no subprocess spawning.
//!   - Spotify: title, artwork URL (downloaded via reqwest).
//!   - Apple Music: title, raw artwork bytes from MusicArtwork.rawData.
//!
//! Linux: Uses `playerctl` subprocess.

/// Debug-only logging (stripped from release builds).
#[allow(unused_macros)]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        eprintln!($($arg)*);
    };
}

pub fn extract_prominent_colors_from_bytes(image_bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
    use auto_palette::{ImageData, Palette, Theme};

    let img = image::load_from_memory(image_bytes).ok()?;
    let rgba = img.to_rgba8();
    let image_data = ImageData::new(rgba.width(), rgba.height(), rgba.as_raw()).ok()?;
    let palette: Palette<f64> = Palette::extract(&image_data).ok()?;

    let swatches = palette
        .find_swatches_with_theme(6, Theme::Vivid)
        .or_else(|_| palette.find_swatches(6))
        .ok()?;

    let colors: Vec<[u8; 3]> = swatches
        .iter()
        .filter(|s| s.color().to_oklch().l > 0.2)
        .take(4)
        .map(|s| {
            let rgb = s.color().to_rgb();
            [rgb.r, rgb.g, rgb.b]
        })
        .collect();

    if colors.is_empty() {
        None
    } else {
        Some(colors)
    }
}

/// Returns the title of the currently playing track.
pub fn get_track_title() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::get_track_title()
    }
    #[cfg(target_os = "linux")]
    {
        linux::get_track_title()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Fetches artwork bytes once and returns both the raw image and the extracted palette.
/// Avoids double-fetch race conditions where the track could change between calls.
pub fn fetch_artwork_and_palette() -> Option<(Vec<u8>, Vec<[u8; 3]>)> {
    #[cfg(target_os = "macos")]
    {
        let bytes = macos::fetch_artwork_bytes()?;
        let colors = macos::extract_colors(&bytes)?;
        Some((bytes, colors))
    }
    #[cfg(target_os = "linux")]
    {
        let bytes = linux::fetch_artwork_bytes()?;
        let colors = linux::extract_colors_from_bytes(&bytes)?;
        Some((bytes, colors))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

// ── macOS — MediaRemote.framework via media-remote crate ─────────────────────

#[cfg(target_os = "macos")]
mod macos {
    pub fn get_track_title() -> Option<String> {
        use media_remote::NowPlayingPerl;
        let np = NowPlayingPerl::new();
        let guard = np.get_info();
        guard.as_ref()?.title.clone()
    }

    pub fn fetch_artwork_bytes() -> Option<Vec<u8>> {
        use media_remote::NowPlayingPerl;
        let np = NowPlayingPerl::new();
        let guard = np.get_info();
        let info = guard.as_ref()?;
        let cover = info.album_cover.as_ref()?;
        let mut buf = std::io::Cursor::new(Vec::new());
        cover.write_to(&mut buf, image::ImageFormat::Jpeg).ok()?;
        Some(buf.into_inner())
    }

    pub fn extract_colors(image_bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
        super::extract_prominent_colors_from_bytes(image_bytes)
    }
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    pub fn get_track_title() -> Option<String> {
        let output = std::process::Command::new("playerctl")
            .args(["metadata", "title"])
            .output()
            .ok()?;
        if output.status.success() {
            let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
        None
    }

    pub fn fetch_artwork_bytes() -> Option<Vec<u8>> {
        let output = std::process::Command::new("playerctl")
            .args(["metadata", "mpris:artUrl"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if url.is_empty() {
            return None;
        }
        if url.starts_with("file://") {
            std::fs::read(url.trim_start_matches("file://")).ok()
        } else {
            reqwest::blocking::get(&url)
                .ok()?
                .bytes()
                .ok()
                .map(|b| b.to_vec())
        }
    }

    pub fn extract_colors_from_bytes(bytes: &[u8]) -> Option<Vec<[u8; 3]>> {
        super::extract_prominent_colors_from_bytes(bytes)
    }
}
